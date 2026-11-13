package http

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

// ErrMissingChannel is returned by [StreamRequest.Read] and the [StreamResponse]
// methods when the invocation carried no stream for them, which happens when the
// function was not invoked by the http worker.
var ErrMissingChannel = errors.New("iii/helpers/http: the invocation carries no stream for this")

// HandlerFunc is an HTTP-style handler for the target of an http::on-request observer,
// the callback of the Node http() helper. Adapt it with [Wrap].
//
// Write the response to res and return nil to stream it, or return a [Response] (any
// non-nil value with status_code, headers and body) to answer buffered without touching
// res. A returned error is reported as the invocation error, which the http worker
// answers with a 500; a response the handler already used is then aborted, so a
// truncated body is not taken for a complete one.
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
	// RequestBody is the raw request body (Node request_body), nil when the invocation
	// carried none. [StreamRequest.Read] reads it; read it another way (its Lines or
	// Frames, say), or pass it on unread in a call payload, instead, not as well.
	RequestBody *iii.InputStream `json:"-"`
}

// Read reads the raw request body and implements [io.Reader]: it is
// [iii.InputStream.Read] on RequestBody. It returns io.EOF only once the http worker
// sent the whole body and closed the stream. If the stream was aborted instead (its
// writer failed or went away before finishing), Read returns an [*iii.InvocationError]
// with code [iii.CodeStreamAborted], never io.EOF: the body read so far is incomplete,
// and io.ReadAll returns it together with the error. Read returns [ErrMissingChannel]
// when RequestBody is nil. A body the handler started reading and did not finish is
// cancelled when it returns.
func (r *StreamRequest) Read(p []byte) (int, error) {
	if r.RequestBody == nil {
		return 0, ErrMissingChannel
	}
	return r.RequestBody.Read(p)
}

// StreamResponse is the response a [HandlerFunc] writes, Node StreamResponse: Status
// and Headers send control messages, Write streams the body and Close ends it.
// StreamResponse is an [io.Writer]. Its methods are safe for concurrent use.
//
// The SDK ends the response when the handler returns: closed when the handler used it
// and succeeded, aborted when it used it and failed. Closing it is optional.
type StreamResponse struct {
	ctx    context.Context
	stream *iii.OutputStream
}

// NewStreamResponse returns a StreamResponse writing to stream, the response stream of
// an http invocation ([InternalRequest].Response). ctx bounds every write. [Wrap] builds
// one for each invocation; call NewStreamResponse only when handling an
// [InternalRequest] by hand. With a nil stream every method returns
// [ErrMissingChannel].
func NewStreamResponse(ctx context.Context, stream *iii.OutputStream) *StreamResponse {
	if ctx == nil {
		ctx = context.Background()
	}
	return &StreamResponse{ctx: ctx, stream: stream}
}

// Stream returns the response stream itself (nil when the invocation carried none),
// for [iii.OutputStream.KeepOpen] or [iii.OutputStream.Abort], say.
func (r *StreamResponse) Stream() *iii.OutputStream { return r.stream }

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
	if r.stream == nil {
		return ErrMissingChannel
	}
	return r.stream.Send(r.ctx, setStatusMessage{Type: "set_status", StatusCode: statusCode})
}

// Headers sets response headers by sending {"type":"set_headers","headers":{...}}.
// Repeated calls merge, later values winning. Like Status, headers must be sent before
// the first body chunk.
func (r *StreamResponse) Headers(headers map[string]string) error {
	if r.stream == nil {
		return ErrMissingChannel
	}
	if headers == nil {
		headers = map[string]string{}
	}
	return r.stream.Send(r.ctx, setHeadersMessage{Type: "set_headers", Headers: headers})
}

// Write streams p as response body (bytes, in frames of at most 64 KiB) and implements
// [io.Writer]. The first body chunk commits the status and headers. A write after Close
// fails with [iii.CodeStreamClosed].
func (r *StreamResponse) Write(p []byte) (int, error) {
	if r.stream == nil {
		return 0, ErrMissingChannel
	}
	if err := r.stream.Write(r.ctx, p); err != nil {
		return 0, err
	}
	return len(p), nil
}

// Close ends the response: the http worker sends the status and headers set so far
// (200 by default) and finishes the body. Close is idempotent, and optional in a
// [Wrap] handler: the SDK closes a response the handler used when it returns.
func (r *StreamResponse) Close() error {
	if r.stream == nil {
		return ErrMissingChannel
	}
	return r.stream.Close()
}

// Wrap adapts an HTTP-style handler into an [iii.Handler] for the target of an
// http::on-request observer; it is the Go counterpart of the Node http() helper.
// Register the result with [iii.Client.RegisterFunction].
//
// For each invocation Wrap decodes the [InternalRequest] and calls fn with the
// [StreamRequest] and [StreamResponse]. The SDK settles the streams when fn returns, as
// it does for every handler:
//   - a response fn used (a status, headers or body sent) is closed when fn succeeded,
//     and aborted when it failed, so a truncated body is not taken for a complete one;
//   - an untouched response is closed when fn returned nil, so the http worker answers
//     with an empty 200, and left alone otherwise, so the http worker answers from the
//     returned value ([Response]) or reports the error;
//   - a request body fn started reading and did not finish is cancelled.
//
// fn must not use req or res after it returns, unless it called KeepOpen on
// res.Stream(). Wrap returns nil when fn is nil.
func Wrap(fn HandlerFunc) iii.Handler {
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
			RequestBody: in.RequestBody,
		}
		return fn(ctx, req, NewStreamResponse(ctx, in.Response))
	}
}
