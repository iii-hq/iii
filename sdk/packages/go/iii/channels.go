package iii

import (
	"context"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/coder/websocket"
)

// This file is the wire side of streams: the WebSocket one end of an engine pipe is read
// or written through. The public stream types (InputStream, OutputStream in streams.go)
// are built on it. The engine keeps the name "channels": a pipe is created with
// engine::channels::create and carried over /ws/channels/<id> sockets.
//
// Framing is the one load-bearing wire rule: messages and bytes share the socket and are
// told apart by WebSocket opcode. A TEXT frame is a message (OutputStream.Send,
// InputStream.Messages); a BINARY frame is bytes (OutputStream.Write, InputStream.Read).
// There is no envelope. End of stream is a close frame relayed writer -> engine ->
// reader. A writer close with code 1000 (or no code) is a completed stream, which the
// reader sees as a close with no code (1005). A writer socket that ends any other way
// (code 1011 from Abort, another code, or no close frame) is an abort: the engine closes
// the reader with 1011 and its own reason, and the reader returns STREAM_ABORTED.

// channelFrameSize is the most bytes one binary frame carries. Matches the 64 KiB
// frames of the Node, Python and Rust SDKs; the engine reassembles by concatenation, so
// the boundary is not semantic, but it keeps backpressure in step with them.
const channelFrameSize = 64 * 1024

// channelFlushDelay is the pause before a writer sends its close frame, giving the TCP
// stack time to flush buffered data frames: without it the close frame can reach the
// engine before the data, which truncates the stream. Same 10ms as the other SDKs.
const channelFlushDelay = 10 * time.Millisecond

// abortCloseCode is the close code an abort sends: the engine relays any code but 1000
// to the reader as an abort.
const abortCloseCode = websocket.StatusInternalError

// maxCloseReason is the longest close reason a WebSocket close frame carries, in bytes.
const maxCloseReason = 123

// streamDialTimeout bounds the dial of a stream socket opened by an operation that
// takes no context: io.Reader reads, Close and Abort.
const streamDialTimeout = 30 * time.Second

// channelURL builds the URL of one end of a pipe from the engine WS base, as every SDK
// does: strip one trailing slash, append /ws/channels/{id}, and add the percent-encoded
// access key plus the raw direction.
func channelURL(engineWSBase, channelID, accessKey string, dir ChannelDirection) string {
	// Keep a query already on the engine URL (e.g. ?token=...); the channel's own come after it.
	base, query, _ := strings.Cut(engineWSBase, "?")
	base = strings.TrimRight(base, "/")
	if query != "" {
		query += "&"
	}
	// Encode the key with the RFC-3986 unreserved set (space -> %20), matching the Rust
	// encoder; url.QueryEscape would emit "+" for space. Access keys are UUIDs, so this
	// only keeps the URL byte-identical to the other SDKs.
	key := strings.ReplaceAll(url.QueryEscape(accessKey), "+", "%20")
	return fmt.Sprintf("%s/ws/channels/%s?%skey=%s&dir=%s", base, channelID, query, key, dir)
}

// closeReason cuts reason to what a close frame carries, on a UTF-8 boundary.
func closeReason(reason string) string {
	if len(reason) <= maxCloseReason {
		return reason
	}
	end := maxCloseReason
	for end > 0 && !utf8.RuneStart(reason[end]) {
		end--
	}
	return reason[:end]
}

// endpoint is where a stream's socket connects: the engine WS base and the handshake
// headers of a client.
type endpoint struct {
	address string
	headers http.Header
}

// streamEndpoint is the endpoint of c: its address, its WithHeaders and the III_TOKEN
// bearer.
func (c *Client) streamEndpoint() *endpoint {
	return &endpoint{address: c.Address(), headers: handshakeHeaders(c.headers)}
}

// dialChannel opens the end of ref described by dir on ep.
func dialChannel(ctx context.Context, ep *endpoint, ref StreamChannelRef, dir ChannelDirection) (*websocket.Conn, error) {
	u := channelURL(ep.address, ref.ChannelID, ref.AccessKey, dir)
	conn, _, err := websocket.Dial(ctx, u, &websocket.DialOptions{HTTPHeader: ep.headers})
	if err != nil {
		return nil, err
	}
	// Frames from SDKs before 0.27 can be larger than the default 32 KiB limit.
	conn.SetReadLimit(-1)
	return conn, nil
}

// FrameType tells the two kinds of frame a stream carries apart.
type FrameType string

const (
	// FrameBinary is bytes: written with [OutputStream.Write], read by
	// [InputStream.Read] and the other byte reads.
	FrameBinary FrameType = "binary"
	// FrameText is a message: sent with [OutputStream.Send], read by
	// [InputStream.Messages].
	FrameText FrameType = "text"
)

// StreamFrame is one frame of a stream, as [InputStream.Frames] yields it: bytes
// (Type [FrameBinary]) or a message (Type [FrameText], Data holding the frame's text).
type StreamFrame struct {
	Type FrameType
	Data []byte
}

// frameSource is where the frames of an input stream come from: the read end of a
// pipe, or a local source. next returns io.EOF at the end; after an error it is not
// called again. cancel releases the source (a socket, a reader that is an io.Closer); a
// next in flight then ends.
type frameSource interface {
	next(ctx context.Context) (StreamFrame, error)
	cancel()
}

// emptySource is a stream with no frames.
type emptySource struct{}

func (emptySource) next(context.Context) (StreamFrame, error) { return StreamFrame{}, io.EOF }
func (emptySource) cancel()                                   {}

// wsSource reads the read end of a pipe, dialled on the first next.
type wsSource struct {
	ep  *endpoint
	ref StreamChannelRef

	mu        sync.Mutex
	conn      *websocket.Conn
	cancelled bool
}

func (s *wsSource) next(ctx context.Context) (StreamFrame, error) {
	s.mu.Lock()
	if s.cancelled {
		s.mu.Unlock()
		return StreamFrame{}, io.EOF
	}
	conn := s.conn
	s.mu.Unlock()
	if conn == nil {
		dialed, err := dialChannel(ctx, s.ep, s.ref, ChannelRead)
		if err != nil {
			if ctx.Err() != nil {
				return StreamFrame{}, ctx.Err()
			}
			// A reader that cannot attach (the engine is gone, or the pipe was released:
			// HTTP 404) never gets the stream: it is incomplete, not empty.
			return StreamFrame{}, streamAbortedError("could not open the stream: " + err.Error())
		}
		s.mu.Lock()
		if s.cancelled {
			s.mu.Unlock()
			go closeReader(dialed)
			return StreamFrame{}, io.EOF
		}
		s.conn = dialed
		s.mu.Unlock()
		conn = dialed
	}
	typ, data, err := conn.Read(ctx)
	if err == nil {
		if typ == websocket.MessageText {
			return StreamFrame{Type: FrameText, Data: data}, nil
		}
		return StreamFrame{Type: FrameBinary, Data: data}, nil
	}
	s.mu.Lock()
	cancelled := s.cancelled
	s.mu.Unlock()
	if cancelled {
		return StreamFrame{}, io.EOF
	}
	return StreamFrame{}, readError(ctx, err)
}

// readError is what a failed read of a pipe's read end means: a close with code 1000,
// or with no code (1005, how the engine ends a clean stream), is the end; any other
// close code, or the connection ending without one, is an abort. A read cut short by
// its own context returns that context's error.
func readError(ctx context.Context, err error) error {
	switch status := websocket.CloseStatus(err); status {
	case websocket.StatusNormalClosure, websocket.StatusNoStatusRcvd:
		return io.EOF
	case -1:
	default:
		var ce websocket.CloseError
		if errors.As(err, &ce) && ce.Reason != "" {
			return streamAbortedError(ce.Reason)
		}
		return streamAbortedError(fmt.Sprintf("channel closed with code %d", int(status)))
	}
	if ctx.Err() != nil {
		return ctx.Err()
	}
	return streamAbortedError("connection lost: " + err.Error())
}

func (s *wsSource) cancel() {
	s.mu.Lock()
	if s.cancelled {
		s.mu.Unlock()
		return
	}
	s.cancelled = true
	conn := s.conn
	s.mu.Unlock()
	if conn != nil {
		go closeReader(conn)
	}
}

// closeReader ends a read socket with a clean close, which tells the engine the reader
// left (it then closes the writer). It runs on its own goroutine: the close handshake
// can wait for the engine's reply.
func closeReader(conn *websocket.Conn) {
	_ = conn.Close(websocket.StatusNormalClosure, "channel_close")
}

// readerSource reads an io.Reader in frames of up to channelFrameSize bytes. A read
// error aborts the stream with its message. The reader is closed, when it is an
// io.Closer, once the stream ends, fails or is cancelled.
type readerSource struct {
	r io.Reader

	mu     sync.Mutex
	eof    bool
	closed bool
}

func (s *readerSource) next(ctx context.Context) (StreamFrame, error) {
	s.mu.Lock()
	eof := s.eof || s.closed
	s.mu.Unlock()
	if eof {
		return StreamFrame{}, io.EOF
	}
	if err := ctx.Err(); err != nil {
		return StreamFrame{}, err
	}
	buf := make([]byte, channelFrameSize)
	for {
		n, err := s.r.Read(buf)
		if n > 0 {
			if err == io.EOF {
				s.mu.Lock()
				s.eof = true
				s.mu.Unlock()
				s.cancel()
			}
			return StreamFrame{Type: FrameBinary, Data: buf[:n]}, nil
		}
		switch {
		case err == io.EOF:
			s.cancel()
			return StreamFrame{}, io.EOF
		case err != nil:
			s.cancel()
			return StreamFrame{}, streamAbortedError(err.Error())
		}
		// A Read of 0 bytes and no error: try again, as io.Reader allows.
	}
}

func (s *readerSource) cancel() {
	s.mu.Lock()
	if s.closed {
		s.mu.Unlock()
		return
	}
	s.closed = true
	s.mu.Unlock()
	if closer, ok := s.r.(io.Closer); ok {
		_ = closer.Close()
	}
}

// bytesSource is a stream of one buffer, in frames of up to channelFrameSize bytes.
type bytesSource struct {
	mu   sync.Mutex
	rest []byte
}

func (s *bytesSource) next(context.Context) (StreamFrame, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if len(s.rest) == 0 {
		return StreamFrame{}, io.EOF
	}
	n := min(len(s.rest), channelFrameSize)
	chunk := s.rest[:n:n]
	s.rest = s.rest[n:]
	return StreamFrame{Type: FrameBinary, Data: chunk}, nil
}

func (s *bytesSource) cancel() {
	s.mu.Lock()
	s.rest = nil
	s.mu.Unlock()
}

// prependSource yields first, then the frames of rest.
type prependSource struct {
	mu    sync.Mutex
	first *StreamFrame
	rest  frameSource
}

func (s *prependSource) next(ctx context.Context) (StreamFrame, error) {
	s.mu.Lock()
	first := s.first
	s.first = nil
	s.mu.Unlock()
	if first != nil {
		return *first, nil
	}
	return s.rest.next(ctx)
}

func (s *prependSource) cancel() { s.rest.cancel() }

// wsWriter is the write end of a pipe: dialled on first use, then frames until it is
// closed or aborted. Its owner (outputInner) serializes the calls.
type wsWriter struct {
	ep  *endpoint
	ref StreamChannelRef

	conn  *websocket.Conn
	ended bool
}

// socket returns the open socket, dialling it on first use.
func (w *wsWriter) socket(ctx context.Context) (*websocket.Conn, error) {
	if w.ended {
		return nil, streamClosedError("the stream is closed: it was closed or aborted")
	}
	if w.conn != nil {
		return w.conn, nil
	}
	conn, err := dialChannel(ctx, w.ep, w.ref, ChannelWrite)
	if err != nil {
		if ctx.Err() != nil {
			return nil, ctx.Err()
		}
		return nil, streamClosedError("could not open the stream: " + err.Error())
	}
	// A writer reads nothing, but the engine's close frame (sent when the reader leaves)
	// and its pings must be answered: CloseRead reads them, so a write after the reader
	// left fails instead of filling the socket buffer.
	conn.CloseRead(context.Background())
	w.conn = conn
	return conn, nil
}

// write sends data in binary frames of at most channelFrameSize bytes; it returns once
// the socket took the last one.
func (w *wsWriter) write(ctx context.Context, data []byte) error {
	conn, err := w.socket(ctx)
	if err != nil {
		return err
	}
	for len(data) > 0 {
		n := min(len(data), channelFrameSize)
		if err := conn.Write(ctx, websocket.MessageBinary, data[:n]); err != nil {
			return w.writeFailed(ctx, conn, err)
		}
		data = data[n:]
	}
	return nil
}

// sendText sends one text frame.
func (w *wsWriter) sendText(ctx context.Context, text []byte) error {
	conn, err := w.socket(ctx)
	if err != nil {
		return err
	}
	if err := conn.Write(ctx, websocket.MessageText, text); err != nil {
		return w.writeFailed(ctx, conn, err)
	}
	return nil
}

// writeFailed ends the socket of a failed write and returns the write's error. The
// socket is dropped without a close frame, which the engine relays to the reader as an
// abort: what it read is not the whole stream. A context that ended mid-write returns
// its error; any other failure means nobody reads what is sent now.
func (w *wsWriter) writeFailed(ctx context.Context, conn *websocket.Conn, err error) error {
	w.ended = true
	w.conn = nil
	_ = conn.CloseNow()
	if ctx.Err() != nil {
		return ctx.Err()
	}
	if websocket.CloseStatus(err) != -1 {
		return streamClosedError("the stream is closed: its reader left")
	}
	return streamClosedError("the stream's connection failed: " + err.Error())
}

// close ends the stream cleanly: a close frame with code 1000, after a short pause so
// the data frames flush first. A socket never dialled is dialled first, so the reader
// sees an empty stream.
func (w *wsWriter) close() error {
	return w.finish(websocket.StatusNormalClosure, "stream_complete", channelFlushDelay)
}

// abort ends the stream with an error: a close frame with code 1011 and reason, which
// the engine relays to the reader as an abort. A socket never dialled is dialled first,
// so the reader always learns of it.
func (w *wsWriter) abort(reason string) error {
	if reason == "" {
		reason = "aborted"
	}
	return w.finish(abortCloseCode, closeReason(reason), 0)
}

func (w *wsWriter) finish(code websocket.StatusCode, reason string, delay time.Duration) error {
	if w.ended {
		return nil
	}
	ctx, cancel := context.WithTimeout(context.Background(), streamDialTimeout)
	defer cancel()
	conn, err := w.socket(ctx)
	w.ended = true
	w.conn = nil
	if err != nil {
		return err
	}
	if delay > 0 {
		time.Sleep(delay)
	}
	// The engine decides on receipt of the close frame, so an error from the wait for its
	// reply, once the frame went out, still ends the stream as asked; and a socket the
	// engine already closed (its reader left) has nobody to tell.
	// shortcut: coder/websocket writes the close frame with a fixed 5 s timeout, so if the
	// engine does not read this socket for that long (a slow reader on a backpressured
	// stream) the frame is lost and the engine reads an abort; upgrade when coder/websocket
	// lets the caller bound the close write.
	_ = conn.Close(code, reason)
	return nil
}
