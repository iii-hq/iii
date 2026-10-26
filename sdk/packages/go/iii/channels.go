package iii

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/coder/websocket"
)

// This file implements iii streaming data channels, ported from
// sdk/packages/node/iii/src/channels.ts and sdk/packages/rust/iii/src/channels.rs.
//
// A channel is a pair of capability-scoped WebSockets, SEPARATE from the main worker
// socket: one to write, one to read. CreateChannel asks the engine for a writer ref and
// a reader ref (sharing a channel_id + access_key); either ref can be embedded as plain
// JSON in a trigger payload so another worker opens the other end.
//
// Framing is the one load-bearing wire rule: discrete messages and streamed bytes share
// the same socket and are told apart purely by WebSocket opcode. A TEXT frame is a
// message (SendMessage / OnMessage); a BINARY frame is stream data (Write / ReadAll).
// There is no envelope. End-of-stream is a Close frame propagated writer -> engine ->
// reader. A writer close with code 1000 (or no code) is a completed stream, which the
// reader sees as a close with no code (1005). A writer socket that ends any other way
// (another code, or no close frame) is an abort: the engine closes the reader with 1011
// and a reason, and the reader returns that as an error.

// channelFrameSize is the max bytes per binary frame. Matches FRAME_SIZE (64 KiB) in
// the Node/Rust SDKs; the engine reassembles by concatenation so the boundary is not
// semantic, but we keep 64 KiB for backpressure parity.
const channelFrameSize = 64 * 1024

// channelFlushDelay is the pause before a writer sends its Close frame, giving the TCP
// stack time to flush buffered data frames. Mirrors the 10ms delay in both reference
// SDKs (channels.ts:47-57, channels.rs:119-130).
const channelFlushDelay = 10 * time.Millisecond

// Channel is a freshly created channel: a live writer/reader plus the refs for each end.
// Pass WriterRef or ReaderRef to another worker (in a trigger payload) so it opens the
// opposite end with [Client.OpenReader] / [Client.OpenWriter]. Created by
// [Client.CreateChannel].
type Channel struct {
	Writer    *ChannelWriter
	Reader    *ChannelReader
	WriterRef StreamChannelRef
	ReaderRef StreamChannelRef
}

// CreateChannel asks the engine to allocate a streaming channel and returns both ends.
// bufferSize bounds the engine's internal queue; nil uses the engine default.
func (c *Client) CreateChannel(ctx context.Context, bufferSize *int) (*Channel, error) {
	payload, err := json.Marshal(struct {
		BufferSize *int `json:"buffer_size"`
	}{BufferSize: bufferSize})
	if err != nil {
		return nil, err
	}
	res, err := c.Trigger(ctx, TriggerRequest{FunctionID: FnCreateChannel, Data: payload})
	if err != nil {
		return nil, err
	}
	var refs struct {
		Writer StreamChannelRef `json:"writer"`
		Reader StreamChannelRef `json:"reader"`
	}
	if err := json.Unmarshal(res, &refs); err != nil {
		return nil, fmt.Errorf("iii: decoding channel refs: %w", err)
	}
	return &Channel{
		Writer:    openWriter(c.Address(), refs.Writer, c.headers),
		Reader:    openReader(c.Address(), refs.Reader, c.headers),
		WriterRef: refs.Writer,
		ReaderRef: refs.Reader,
	}, nil
}

// channelURL builds the channel WebSocket URL from the engine WS base, mirroring
// buildChannelUrl (channels.ts:243, channels.rs:41): strip one trailing slash, append
// /ws/channels/{id}, and add the percent-encoded access key plus the raw direction.
func channelURL(engineWSBase, channelID, accessKey string, dir ChannelDirection) string {
	// Keep a query already on the engine URL (e.g. ?token=...); the channel's own come after it.
	base, query, _ := strings.Cut(engineWSBase, "?")
	base = strings.TrimRight(base, "/")
	if query != "" {
		query += "&"
	}
	// Encode the key with the RFC-3986 unreserved set (space -> %20), matching the Rust
	// encoder; url.QueryEscape would emit "+" for space. Access keys are UUIDs so this
	// is belt-and-suspenders, but it keeps us byte-identical to the references.
	key := strings.ReplaceAll(url.QueryEscape(accessKey), "+", "%20")
	return fmt.Sprintf("%s/ws/channels/%s?%skey=%s&dir=%s", base, channelID, query, key, dir)
}

// ChannelWriter is the write end of a channel: a WebSocket opened lazily on first use.
// SendMessage emits a text frame; Write emits 64 KiB-chunked binary frames. Close
// finishes the stream. A ChannelWriter is safe for serial use by one goroutine; it is
// not designed for concurrent writers (mirroring the reference SDKs).
type ChannelWriter struct {
	url     string
	headers http.Header

	mu   sync.Mutex
	conn *websocket.Conn

	failed atomic.Bool // a send failed, so Close must not report the stream complete
}

// OpenWriter returns a writer for the write end described by ref, against the given
// engine WS base (typically Client.Address()). The socket connects on first send and
// authenticates with III_TOKEN, when set; channels from [Client.CreateChannel] also send
// the client's [WithHeaders]. Prefer [Client.OpenWriter], which uses the client's
// address and headers.
func OpenWriter(engineWSBase string, ref StreamChannelRef) *ChannelWriter {
	return openWriter(engineWSBase, ref, nil)
}

// OpenWriter returns a writer for the write end described by ref (typically received in
// a trigger payload), against this client's engine address. Like the ends returned by
// [Client.CreateChannel], its handshake sends the client's [WithHeaders] plus
// "Authorization: Bearer $III_TOKEN" when that variable is set and the headers have no
// Authorization header. The socket connects on first send.
func (c *Client) OpenWriter(ref StreamChannelRef) *ChannelWriter {
	return openWriter(c.Address(), ref, c.headers)
}

func openWriter(engineWSBase string, ref StreamChannelRef, headers http.Header) *ChannelWriter {
	return &ChannelWriter{
		url:     channelURL(engineWSBase, ref.ChannelID, ref.AccessKey, ChannelWrite),
		headers: handshakeHeaders(headers),
	}
}

func (w *ChannelWriter) ensureConnected(ctx context.Context) (*websocket.Conn, error) {
	w.mu.Lock()
	defer w.mu.Unlock()
	if w.conn != nil {
		return w.conn, nil
	}
	conn, _, err := websocket.Dial(ctx, w.url, &websocket.DialOptions{HTTPHeader: w.headers})
	if err != nil {
		return nil, fmt.Errorf("iii: dialing channel writer: %w", err)
	}
	conn.SetReadLimit(-1)
	w.conn = conn
	return conn, nil
}

// SendMessage sends a discrete text message (a TEXT frame) on the channel.
func (w *ChannelWriter) SendMessage(ctx context.Context, msg string) error {
	conn, err := w.ensureConnected(ctx)
	if err != nil {
		return err
	}
	if err := conn.Write(ctx, websocket.MessageText, []byte(msg)); err != nil {
		w.failed.Store(true)
		return err
	}
	return nil
}

// Write streams bytes as BINARY frames, chunked at channelFrameSize. It implements the
// stream-data half of the channel; the reader receives the bytes via NextBinary/ReadAll.
func (w *ChannelWriter) Write(ctx context.Context, data []byte) error {
	conn, err := w.ensureConnected(ctx)
	if err != nil {
		return err
	}
	for len(data) > 0 {
		n := len(data)
		if n > channelFrameSize {
			n = channelFrameSize
		}
		if err := conn.Write(ctx, websocket.MessageBinary, data[:n]); err != nil {
			w.failed.Store(true)
			return err
		}
		data = data[n:]
	}
	return nil
}

// Close finishes the stream: it pauses briefly so buffered frames flush, then closes the
// socket with code 1000, which the engine treats as a completed stream (the reader sees
// EOF). After a failed Write or SendMessage the stream is incomplete, so Close ends the
// socket without a close frame instead: the engine reads that as an abort and the reader
// gets an error, as it does when the process exits before Close. Close is idempotent.
func (w *ChannelWriter) Close() error {
	w.mu.Lock()
	conn := w.conn
	w.conn = nil
	w.mu.Unlock()
	if conn == nil {
		return nil
	}
	if w.failed.Load() {
		return conn.CloseNow()
	}
	time.Sleep(channelFlushDelay)
	// Code 1000 is what tells the engine the stream completed. The engine decides on receipt
	// of the frame, so an error from the reply wait after it went out is still a clean end.
	// shortcut: coder/websocket writes the close frame with a fixed 5 s timeout, so if the
	// engine does not read this socket for that long (a slow or late reader on a
	// backpressured stream) the frame is lost, the engine reads an abort and Close still
	// returns nil; upgrade when coder/websocket lets the caller bound the close write.
	return conn.Close(websocket.StatusNormalClosure, "stream_complete")
}

// ChannelReader is the read end of a channel: a WebSocket opened lazily on first use.
// Binary frames are stream data (NextBinary / ReadAll); text frames are messages
// delivered to OnMessage callbacks. The reader reads from a single goroutine internally,
// so register OnMessage callbacks before the first NextBinary/ReadAll call.
type ChannelReader struct {
	url     string
	headers http.Header

	mu         sync.Mutex
	conn       *websocket.Conn
	onMessage  []func(string)
	textBuffer []string // text frames seen while waiting on NextBinary, drained to callbacks

	closed atomic.Bool // set by Close: reads end as end of stream and do not redial
}

// OpenReader returns a reader for the read end described by ref, against the given engine
// WS base (typically Client.Address()). The socket connects on first read and
// authenticates with III_TOKEN, when set; channels from [Client.CreateChannel] also send
// the client's [WithHeaders]. Prefer [Client.OpenReader], which uses the client's
// address and headers.
func OpenReader(engineWSBase string, ref StreamChannelRef) *ChannelReader {
	return openReader(engineWSBase, ref, nil)
}

// OpenReader returns a reader for the read end described by ref (typically received in
// a trigger payload), against this client's engine address. Like the ends returned by
// [Client.CreateChannel], its handshake sends the client's [WithHeaders] plus
// "Authorization: Bearer $III_TOKEN" when that variable is set and the headers have no
// Authorization header. The socket connects on first read.
func (c *Client) OpenReader(ref StreamChannelRef) *ChannelReader {
	return openReader(c.Address(), ref, c.headers)
}

func openReader(engineWSBase string, ref StreamChannelRef, headers http.Header) *ChannelReader {
	return &ChannelReader{
		url:     channelURL(engineWSBase, ref.ChannelID, ref.AccessKey, ChannelRead),
		headers: handshakeHeaders(headers),
	}
}

// OnMessage registers a callback for inbound text messages. Callbacks fire from the
// goroutine driving NextBinary/ReadAll, in registration order.
func (r *ChannelReader) OnMessage(cb func(string)) {
	r.mu.Lock()
	r.onMessage = append(r.onMessage, cb)
	r.mu.Unlock()
}

func (r *ChannelReader) ensureConnected(ctx context.Context) (*websocket.Conn, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.conn != nil {
		return r.conn, nil
	}
	if r.closed.Load() {
		return nil, nil
	}
	conn, _, err := websocket.Dial(ctx, r.url, &websocket.DialOptions{HTTPHeader: r.headers})
	if err != nil {
		return nil, fmt.Errorf("iii: dialing channel reader: %w", err)
	}
	conn.SetReadLimit(-1)
	r.conn = conn
	return conn, nil
}

// NextBinary returns the next binary stream chunk. ok is false at end of stream (the
// writer closed cleanly, or this reader was closed). If the writer aborted or the
// connection dropped, err is non-nil; a close carries its code and reason
// (websocket.CloseStatus, errors.As with websocket.CloseError). Text frames encountered
// while waiting are dispatched to OnMessage callbacks and skipped, so a reader can
// interleave messages and stream data.
func (r *ChannelReader) NextBinary(ctx context.Context) (data []byte, ok bool, err error) {
	conn, err := r.ensureConnected(ctx)
	if conn == nil {
		return nil, false, err // a nil err here means the reader was closed: end of stream
	}
	for {
		typ, frame, readErr := conn.Read(ctx)
		if readErr != nil {
			// A read cut short by the caller's own Close is end of stream, not an error.
			if r.closed.Load() {
				return nil, false, nil
			}
			// Only 1000 or a close with no code (1005, the engine's clean end) is end of
			// stream; any other code (1011 for an aborted writer) is an error.
			switch websocket.CloseStatus(readErr) {
			case websocket.StatusNormalClosure, websocket.StatusNoStatusRcvd:
				return nil, false, nil
			}
			return nil, false, readErr
		}
		switch typ {
		case websocket.MessageBinary:
			return frame, true, nil
		case websocket.MessageText:
			r.dispatchText(string(frame))
			// keep reading until a binary frame or close
		}
	}
}

// dispatchText delivers a text frame to all registered OnMessage callbacks.
func (r *ChannelReader) dispatchText(msg string) {
	r.mu.Lock()
	cbs := make([]func(string), len(r.onMessage))
	copy(cbs, r.onMessage)
	r.mu.Unlock()
	for _, cb := range cbs {
		cb(msg)
	}
}

// ReadAll consumes the channel until the writer closes, concatenating all binary chunks.
// Text messages seen along the way are dispatched to OnMessage callbacks. On an error (an
// aborted writer included) it returns the bytes read so far with the error.
func (r *ChannelReader) ReadAll(ctx context.Context) ([]byte, error) {
	var out []byte
	for {
		chunk, ok, err := r.NextBinary(ctx)
		if err != nil {
			return out, err
		}
		if !ok {
			return out, nil
		}
		out = append(out, chunk...)
	}
}

// Close closes the reader socket. A read it cuts short, and any read after it, ends as end
// of stream, not an error. It is idempotent and safe to call after ReadAll.
func (r *ChannelReader) Close() error {
	r.mu.Lock()
	r.closed.Store(true)
	conn := r.conn
	r.conn = nil
	r.mu.Unlock()
	if conn == nil {
		return nil
	}
	return conn.Close(websocket.StatusNormalClosure, "channel_close")
}

// ExtractChannelRefs recursively walks a JSON value and returns every StreamChannelRef
// found, keyed by its dotted path (e.g. "reader", "data.writer"). A node is a ref iff it
// is an object with string channel_id, access_key, and direction fields. Mirrors
// extract_channel_refs and is_channel_ref in the Rust SDK: the receiving worker uses it
// to find a ref another worker embedded in a trigger payload.
func ExtractChannelRefs(data json.RawMessage) (map[string]StreamChannelRef, error) {
	var v any
	if err := json.Unmarshal(data, &v); err != nil {
		return nil, err
	}
	out := map[string]StreamChannelRef{}
	walkChannelRefs("", v, out)
	return out, nil
}

func walkChannelRefs(path string, v any, out map[string]StreamChannelRef) {
	switch node := v.(type) {
	case map[string]any:
		if ref, ok := asChannelRef(node); ok {
			out[path] = ref
			return // a ref is a leaf; don't descend into its own fields
		}
		for k, child := range node {
			walkChannelRefs(joinPath(path, k), child, out)
		}
	case []any:
		for i, child := range node {
			walkChannelRefs(joinPath(path, fmt.Sprintf("%d", i)), child, out)
		}
	}
}

// asChannelRef reports whether node has the three string fields of a StreamChannelRef and
// returns the parsed ref.
func asChannelRef(node map[string]any) (StreamChannelRef, bool) {
	id, ok1 := node["channel_id"].(string)
	key, ok2 := node["access_key"].(string)
	dir, ok3 := node["direction"].(string)
	if !(ok1 && ok2 && ok3) {
		return StreamChannelRef{}, false
	}
	return StreamChannelRef{ChannelID: id, AccessKey: key, Direction: ChannelDirection(dir)}, true
}

func joinPath(base, key string) string {
	if base == "" {
		return key
	}
	return base + "." + key
}
