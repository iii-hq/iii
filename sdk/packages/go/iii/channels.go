package iii

import (
	"context"
	"encoding/json"
	"fmt"
	"net/url"
	"strings"
	"sync"
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
// Framing — the one load-bearing wire rule: discrete messages and streamed bytes share
// the same socket and are told apart purely by WebSocket opcode. A TEXT frame is a
// message (SendMessage / OnMessage); a BINARY frame is stream data (Write / ReadAll).
// There is no envelope. End-of-stream is a Close frame propagated writer -> engine ->
// reader (the engine ignores the close code/reason; only the opcode matters).

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
// opposite end with OpenReader / OpenWriter.
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
		Writer:    OpenWriter(c.Address(), refs.Writer),
		Reader:    OpenReader(c.Address(), refs.Reader),
		WriterRef: refs.Writer,
		ReaderRef: refs.Reader,
	}, nil
}

// channelURL builds the channel WebSocket URL from the engine WS base, mirroring
// buildChannelUrl (channels.ts:243, channels.rs:41): strip one trailing slash, append
// /ws/channels/{id}, and add the percent-encoded access key plus the raw direction.
func channelURL(engineWSBase, channelID, accessKey string, dir ChannelDirection) string {
	base := strings.TrimRight(engineWSBase, "/")
	// Encode the key with the RFC-3986 unreserved set (space -> %20), matching the Rust
	// encoder; url.QueryEscape would emit "+" for space. Access keys are UUIDs so this
	// is belt-and-suspenders, but it keeps us byte-identical to the references.
	key := strings.ReplaceAll(url.QueryEscape(accessKey), "+", "%20")
	return fmt.Sprintf("%s/ws/channels/%s?key=%s&dir=%s", base, channelID, key, dir)
}

// ChannelWriter is the write end of a channel: a WebSocket opened lazily on first use.
// SendMessage emits a text frame; Write emits 64 KiB-chunked binary frames. Close
// finishes the stream. A ChannelWriter is safe for serial use by one goroutine; it is
// not designed for concurrent writers (mirroring the reference SDKs).
type ChannelWriter struct {
	url string

	mu   sync.Mutex
	conn *websocket.Conn
}

// OpenWriter returns a writer for the write end described by ref, against the given
// engine WS base (typically Client.Address()). The socket connects on first send.
func OpenWriter(engineWSBase string, ref StreamChannelRef) *ChannelWriter {
	return &ChannelWriter{url: channelURL(engineWSBase, ref.ChannelID, ref.AccessKey, ChannelWrite)}
}

func (w *ChannelWriter) ensureConnected(ctx context.Context) (*websocket.Conn, error) {
	w.mu.Lock()
	defer w.mu.Unlock()
	if w.conn != nil {
		return w.conn, nil
	}
	conn, _, err := websocket.Dial(ctx, w.url, nil)
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
	return conn.Write(ctx, websocket.MessageText, []byte(msg))
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
			return err
		}
		data = data[n:]
	}
	return nil
}

// Close finishes the stream: it pauses briefly so buffered frames flush, then closes the
// socket. The engine treats the close as end-of-stream and propagates it to the reader.
// Close is idempotent.
func (w *ChannelWriter) Close() error {
	w.mu.Lock()
	conn := w.conn
	w.conn = nil
	w.mu.Unlock()
	if conn == nil {
		return nil
	}
	time.Sleep(channelFlushDelay)
	// The close code/reason are ignored by the engine; the opcode is what signals
	// completion. Close sends the close frame and waits for the peer's reply; bound that
	// wait so a slow or absent peer can't hang the caller, falling back to an abrupt
	// close. Either way the close frame reaches the engine, which is the end-of-stream
	// signal.
	if err := conn.Close(websocket.StatusNormalClosure, "stream_complete"); err != nil {
		return conn.CloseNow()
	}
	return nil
}

// ChannelReader is the read end of a channel: a WebSocket opened lazily on first use.
// Binary frames are stream data (NextBinary / ReadAll); text frames are messages
// delivered to OnMessage callbacks. The reader reads from a single goroutine internally,
// so register OnMessage callbacks before the first NextBinary/ReadAll call.
type ChannelReader struct {
	url string

	mu         sync.Mutex
	conn       *websocket.Conn
	onMessage  []func(string)
	textBuffer []string // text frames seen while waiting on NextBinary, drained to callbacks
}

// OpenReader returns a reader for the read end described by ref, against the given engine
// WS base (typically Client.Address()). The socket connects on first read.
func OpenReader(engineWSBase string, ref StreamChannelRef) *ChannelReader {
	return &ChannelReader{url: channelURL(engineWSBase, ref.ChannelID, ref.AccessKey, ChannelRead)}
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
	conn, _, err := websocket.Dial(ctx, r.url, nil)
	if err != nil {
		return nil, fmt.Errorf("iii: dialing channel reader: %w", err)
	}
	conn.SetReadLimit(-1)
	r.conn = conn
	return conn, nil
}

// NextBinary returns the next binary stream chunk. ok is false at end of stream (the
// writer closed). Text frames encountered while waiting are dispatched to OnMessage
// callbacks and skipped, so a reader can interleave messages and stream data.
func (r *ChannelReader) NextBinary(ctx context.Context) (data []byte, ok bool, err error) {
	conn, err := r.ensureConnected(ctx)
	if err != nil {
		return nil, false, err
	}
	for {
		typ, frame, readErr := conn.Read(ctx)
		if readErr != nil {
			// A normal closure (or any close) is end-of-stream, not an error.
			if websocket.CloseStatus(readErr) != -1 {
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
// Text messages seen along the way are dispatched to OnMessage callbacks.
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

// Close closes the reader socket. It is idempotent and safe to call after ReadAll.
func (r *ChannelReader) Close() error {
	r.mu.Lock()
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
// extract_channel_refs / is_channel_ref (channels.rs:225-275): the receiving worker uses
// it to find a ref another worker embedded in a trigger payload.
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
