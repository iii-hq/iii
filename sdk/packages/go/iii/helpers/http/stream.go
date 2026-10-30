package http

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"reflect"
	"sync"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

var (
	// ErrMissingChannel is returned by [StreamRequest.Read] and the [StreamResponse]
	// methods when the invocation carried no channel ref for that stream, which happens
	// when the function was not invoked by the http worker.
	ErrMissingChannel = errors.New("iii/helpers/http: the invocation carries no channel for this stream")
	// ErrResponseClosed is returned by the [StreamResponse] write methods after
	// [StreamResponse.Close].
	ErrResponseClosed = errors.New("iii/helpers/http: the response is closed")
)

// HandlerFunc is an HTTP-style handler for a function behind an http trigger, the
// callback of the Node http() helper. Adapt it with [Wrap].
//
// Write the response to res and return nil to stream it, or return a [Response] (any
// non-nil value with status_code, headers and body) to answer buffered without touching
// res. A returned error is reported as the invocation error, which the http worker
// answers with a 500; if body bytes were already streamed, [Wrap] aborts the response
// instead of ending it, so the truncated body is not taken for a complete one.
type HandlerFunc func(ctx context.Context, req *StreamRequest, res *StreamResponse) (any, error)

// StreamRequest is the incoming request a [HandlerFunc] receives, Node StreamRequest:
// the parsed request plus the raw body as a stream. StreamRequest is an [io.Reader] over
// the raw request body.
type StreamRequest struct {
	// PathParams holds the path parameters extracted from the matched route.
	PathParams map[string]string `json:"path_params"`
	// QueryParams holds the query string parameters of the request URL.
	QueryParams map[string]MultiValue `json:"query_params"`
	// Body is the request body as parsed by the http worker, as raw JSON.
	Body json.RawMessage `json:"body"`
	// Headers holds the request headers.
	Headers map[string]MultiValue `json:"headers"`
	// Path is the route template that matched. The Node type does not declare it; the
	// http worker sends it.
	Path string `json:"path,omitempty"`
	// Method is the HTTP method of the request.
	Method string `json:"method"`
	// RequestBody is the channel reader carrying the raw request body (Node
	// request_body), nil when the invocation carried none. [StreamRequest.Read] reads
	// from it; use it directly only for OnMessage, and do not mix the two.
	RequestBody *iii.ChannelReader `json:"-"`

	ctx     context.Context
	pending []byte
	eof     bool
	err     error // the error that ended the body, returned by every later Read
}

// Read reads the raw request body from RequestBody and implements [io.Reader]. It
// returns io.EOF only once the http worker has sent the whole body and ended the channel
// cleanly. If the channel was aborted instead (its writer failed or went away before
// finishing, or the connection dropped), Read returns that error, never io.EOF: the body
// read so far is incomplete, and io.ReadAll returns it together with the error. An abort
// reported by the engine is a websocket close error with code 1011 and a reason
// (websocket.CloseStatus reads the code). Bytes received before the abort are returned
// first, and once Read failed every later call returns the same error. Read returns
// [ErrMissingChannel] when RequestBody is nil. Reads are bound to the invocation
// context; a StreamRequest is not safe for concurrent reads.
func (r *StreamRequest) Read(p []byte) (int, error) {
	if r.RequestBody == nil {
		return 0, ErrMissingChannel
	}
	if len(p) == 0 {
		return 0, nil
	}
	for len(r.pending) == 0 {
		switch {
		case r.err != nil:
			return 0, r.err
		case r.eof:
			return 0, io.EOF
		}
		chunk, ok, err := r.RequestBody.NextBinary(contextOrBackground(r.ctx))
		switch {
		case err != nil:
			r.err = err
		case !ok:
			r.eof = true
		default:
			r.pending = chunk
		}
	}
	n := copy(p, r.pending)
	r.pending = r.pending[n:]
	return n, nil
}

// StreamResponse is the response a [HandlerFunc] writes, Node StreamResponse: Status
// and Headers send control messages, Write streams the body and Close ends it.
// StreamResponse is an [io.Writer]. Its methods are safe for concurrent use.
//
// A response whose writer failed is aborted, never reported complete. Once Status,
// Headers or Write returned an error, every later call returns that error, and Close
// (or [Wrap], when the handler returns) ends the channel without a clean close: the http
// worker's reader gets an error instead of a truncated body that looks whole.
type StreamResponse struct {
	ctx    context.Context
	writer *iii.ChannelWriter

	mu        sync.Mutex
	opened    bool  // a frame was sent, so the channel socket is open
	wroteBody bool  // body bytes were written: the http worker is streaming
	closed    bool  // ended: closed cleanly, aborted, or dropped by Wrap
	err       error // the first failed send: the response can only be aborted
}

// NewStreamResponse returns a StreamResponse writing to writer, the response channel of
// an http invocation opened with [iii.Client.OpenWriter]. ctx bounds the channel dial and every
// write. [Wrap] builds one for each invocation; call NewStreamResponse only when
// handling an [InternalRequest] by hand. With a nil writer every method returns
// [ErrMissingChannel].
func NewStreamResponse(ctx context.Context, writer *iii.ChannelWriter) *StreamResponse {
	return &StreamResponse{ctx: ctx, writer: writer}
}

// setStatusMessage and setHeadersMessage are the control messages of the Node http()
// helper (helpers/src/http/index.ts), parsed by the http worker's ControlMessage.
type setStatusMessage struct {
	Type       string `json:"type"`
	StatusCode int    `json:"status_code"`
}

type setHeadersMessage struct {
	Type    string            `json:"type"`
	Headers map[string]string `json:"headers"`
}

// Status sets the HTTP status code by sending {"type":"set_status","status_code":N}.
// The http worker applies it when it arrives before the first body chunk and ignores
// codes outside 100-599.
func (r *StreamResponse) Status(statusCode int) error {
	return r.sendControl(setStatusMessage{Type: "set_status", StatusCode: statusCode})
}

// Headers sets response headers by sending {"type":"set_headers","headers":{...}}.
// Repeated calls merge, later values winning. Like Status, headers must be sent before
// the first body chunk.
func (r *StreamResponse) Headers(headers map[string]string) error {
	if headers == nil {
		headers = map[string]string{}
	}
	return r.sendControl(setHeadersMessage{Type: "set_headers", Headers: headers})
}

func (r *StreamResponse) sendControl(msg any) error {
	text, err := json.Marshal(msg)
	if err != nil {
		return err
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	if err := r.checkWritableLocked(); err != nil {
		return err
	}
	if err := r.writer.SendMessage(contextOrBackground(r.ctx), string(text)); err != nil {
		r.err = err
		return err
	}
	r.opened = true
	return nil
}

// Write streams p as response body (BINARY frames of at most 64 KiB) and implements
// [io.Writer]. The first body chunk commits the status and headers. A failed Write
// leaves the body incomplete, so the response can then only be aborted.
func (r *StreamResponse) Write(p []byte) (int, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if err := r.checkWritableLocked(); err != nil {
		return 0, err
	}
	if len(p) > 0 {
		r.wroteBody = true
	}
	if err := r.writer.Write(contextOrBackground(r.ctx), p); err != nil {
		r.err = err
		return 0, err
	}
	r.opened = true
	return len(p), nil
}

// Close ends the response: the http worker sends the status and headers set so far
// (200 by default) and finishes the body. After a failed Status, Headers or Write, Close
// aborts the response instead: the channel ends without a clean close, the http worker's
// reader gets an error, and Close returns nil (the failing call already returned the
// error). Close is idempotent; [Wrap] ends the response when the handler returns, so
// calling it is optional there.
func (r *StreamResponse) Close() error {
	r.mu.Lock()
	defer r.mu.Unlock()
	switch {
	case r.closed:
		return nil
	case r.writer == nil:
		return ErrMissingChannel
	case r.err != nil:
		r.abortLocked()
		return nil
	}
	return r.closeLocked()
}

func (r *StreamResponse) checkWritableLocked() error {
	switch {
	case r.err != nil:
		return r.err
	case r.closed:
		return ErrResponseClosed
	case r.writer == nil:
		return ErrMissingChannel
	}
	return nil
}

// abortLocked ends a response whose send failed. The channel writer recorded the
// failure, so its Close ends the socket without a close frame, which the engine reads as
// an abort; Close never dials, so a channel that never connected is left untouched.
func (r *StreamResponse) abortLocked() {
	if r.closed {
		return
	}
	r.closed = true
	_ = r.writer.Close()
}

// closeLocked ends the stream. The channel socket opens lazily, so a response that
// sent nothing yet dials it first: the close frame is the end-of-stream signal the http
// worker waits for.
func (r *StreamResponse) closeLocked() error {
	if r.closed {
		return nil
	}
	r.closed = true
	if !r.opened {
		if err := r.writer.Write(contextOrBackground(r.ctx), nil); err != nil {
			return err
		}
		r.opened = true
	}
	return r.writer.Close()
}

// finish runs after the handler returned:
//   - a send failed: the response is aborted;
//   - the handler failed after body bytes were written: the response is aborted with
//     abortWriter;
//   - body bytes were written, or the result is nil without error: the stream is ended
//     cleanly, since the answer is on the channel (possibly with no body);
//   - otherwise (a buffered result, or an error before any body) the channel is left
//     alone, as the Node helper does, so the http worker answers from the returned value
//     or reports the error instead of racing an end of stream.
func (r *StreamResponse) finish(nilResult bool, failed bool) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.writer == nil || r.closed {
		return
	}
	switch {
	case r.err != nil:
		r.abortLocked()
	case failed && r.wroteBody:
		r.closed = true
		abortWriter(r.writer) // never report this truncated body complete
	case r.wroteBody || (nilResult && !failed):
		_ = r.closeLocked()
	}
}

// abortMessage is the text frame abortWriter sends with a cancelled context. If it gets
// through, the http worker skips it as an unknown control message.
const abortMessage = `{"type":"abort"}`

// abortWriter ends w's socket without a clean close, which the engine reports to the
// reader as an abort. iii.ChannelWriter has no exported abort, so it sends with an
// already cancelled context, which can only end one of two ways: the send fails, and
// Close after a failed send ends the socket with no close frame; or the write started,
// and the websocket library closes the connection with no close frame because the
// context is done (context.AfterFunc on a done context always runs). Close is not called
// in the second case, since the socket is already being torn down.
func abortWriter(w *iii.ChannelWriter) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if err := w.SendMessage(ctx, abortMessage); err != nil {
		_ = w.Close()
	}
}

// Wrap adapts an HTTP-style handler into an [iii.Handler] for a function behind an http
// trigger; it is the Go counterpart of the Node http() helper. Register the result with
// [iii.Client.RegisterFunction].
//
// For each invocation Wrap decodes the [InternalRequest], opens the request body channel
// for reading and the response channel for writing with [iii.Client.OpenReader] and
// [iii.Client.OpenWriter] (so the dials use the client's engine address and send its
// [iii.WithHeaders], plus III_TOKEN), and calls fn with the [StreamRequest] and
// [StreamResponse]. When fn returns, Wrap closes the request body reader and finishes
// the response like a net/http handler:
//   - a streamed body, or a nil result without error, ends the response stream cleanly
//     (no explicit Close needed);
//   - a buffered result, or an error before any body, leaves the channel untouched so
//     the http worker answers from the returned value or reports the error;
//   - a failed Status, Headers or Write aborts the response, whatever fn returns;
//   - an error after body bytes were written aborts the response too, so the truncated
//     body is not taken for a complete one.
//
// fn must not use req or res after it returns. Wrap returns nil when fn is nil; it
// panics when client is nil.
func Wrap(client *iii.Client, fn HandlerFunc) iii.Handler {
	if client == nil {
		panic("iii/helpers/http: Wrap requires a non-nil client")
	}
	if fn == nil {
		return nil
	}
	return func(ctx context.Context, data json.RawMessage) (any, error) {
		var in InternalRequest
		if len(bytes.TrimSpace(data)) > 0 {
			if err := json.Unmarshal(data, &in); err != nil {
				return nil, fmt.Errorf("iii/helpers/http: decoding the http request: %w", err)
			}
		}
		req := &StreamRequest{
			PathParams:  in.PathParams,
			QueryParams: in.QueryParams,
			Body:        in.Body,
			Headers:     in.Headers,
			Path:        in.Path,
			Method:      in.Method,
			ctx:         ctx,
		}
		if in.RequestBody.ChannelID != "" {
			req.RequestBody = client.OpenReader(in.RequestBody)
			defer func() { _ = req.RequestBody.Close() }()
		}
		var writer *iii.ChannelWriter
		if in.Response.ChannelID != "" {
			writer = client.OpenWriter(in.Response)
		}
		res := NewStreamResponse(ctx, writer)

		result, err := fn(ctx, req, res)
		res.finish(isNilResult(result), err != nil)
		return result, err
	}
}

// isNilResult reports whether a handler result encodes as JSON null, which the http
// worker reads as "the response is on the channel".
func isNilResult(v any) bool {
	if v == nil {
		return true
	}
	if raw, ok := v.(json.RawMessage); ok {
		trimmed := bytes.TrimSpace(raw)
		return len(trimmed) == 0 || bytes.Equal(trimmed, []byte("null"))
	}
	rv := reflect.ValueOf(v)
	switch rv.Kind() {
	case reflect.Pointer, reflect.Map, reflect.Slice, reflect.Interface:
		return rv.IsNil()
	default:
		return false
	}
}

func contextOrBackground(ctx context.Context) context.Context {
	if ctx == nil {
		return context.Background()
	}
	return ctx
}
