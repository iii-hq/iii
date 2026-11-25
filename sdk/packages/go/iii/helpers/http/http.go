// Package http holds the HTTP helpers of the iii Go SDK: the Go counterpart of the Node
// @iii-dev/helpers/http module (plus the StreamRequest and StreamResponse types of the
// Node SDK) and of the Rust iii_helpers::http module.
//
// It covers two kinds of functions:
//
//   - Functions that observe http::on-request (the target of an observer with route
//     args). The http worker calls them with an [InternalRequest]: the parsed request
//     plus two streams, request_body (an [iii.InputStream] of the raw request body) and
//     response (an [iii.OutputStream] for the status, the headers and the body). [Wrap]
//     adapts a handler that takes a [StreamRequest] and a [StreamResponse] into an
//     [iii.Handler], like the Node http() helper. Handlers that only need the parsed
//     body can decode a [Request] and return a [Response] instead.
//   - Functions the engine itself invokes over HTTP (Lambda, Cloudflare Workers and
//     similar), described by an [InvocationConfig] and the [Method] constants.
//
// # Response stream protocol
//
// On the response stream a message is a control message, either
// {"type":"set_status","status_code":201} or
// {"type":"set_headers","headers":{"content-type":"text/plain"}}; bytes are the body,
// and closing the stream ends the response. The status and headers are committed when
// the first body chunk (or the close) reaches the http worker. A function that returns a
// non-null value without touching the response gets a buffered response built from that
// value ({status_code, headers, body}) instead.
//
// The SDK ends the response when the handler returns, as it does every stream a handler
// receives: a response the handler used (a status, headers or body sent) is closed when
// it succeeded and aborted when it failed, so the http worker never takes a truncated
// body for a whole one; an untouched response is closed only when the handler returned
// nil, and left to the http worker otherwise. An aborted request body makes
// [StreamRequest.Read] return an error with code [iii.CodeStreamAborted], never io.EOF.
//
// Import the package under an alias to keep net/http available:
//
//	import iiihttp "github.com/iii-hq/iii/sdk/packages/go/iii/helpers/http"
package http

import (
	"bytes"
	"encoding/json"
	"fmt"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

// Method is an HTTP method accepted by [InvocationConfig] (Node HttpMethod). It is an
// alias of string so the constants assign directly to [iii.HTTPInvocationRef].Method.
// It is distinct from the method arg of an http::on-request observer, which also
// accepts HEAD and OPTIONS.
type Method = string

// HTTP methods accepted by [InvocationConfig]. An empty Method means [MethodPost].
const (
	// MethodGet is the GET method.
	MethodGet Method = "GET"
	// MethodPost is the POST method, the default.
	MethodPost Method = "POST"
	// MethodPut is the PUT method.
	MethodPut Method = "PUT"
	// MethodPatch is the PATCH method.
	MethodPatch Method = "PATCH"
	// MethodDelete is the DELETE method.
	MethodDelete Method = "DELETE"
)

// InvocationConfig configures a function the engine invokes over HTTP (Lambda,
// Cloudflare Workers and similar), Node HttpInvocationConfig. It is the core
// [iii.HTTPInvocationRef]: URL, Method (POST when empty), TimeoutMs, Headers, and Auth,
// the JSON of a Node HttpAuthConfig: {"type":"hmac","secret_key":...},
// {"type":"bearer","token_key":...} or {"type":"api_key","header":...,"value_key":...}.
//
// Deprecated: HTTP-invoked functions (iii-http-functions) will be removed in an
// upcoming release; behavior is unchanged for now. Register a handler that makes
// the HTTP call instead. Migration guide:
// https://iii.dev/docs/upgrading/migrate-from-http-functions
type InvocationConfig = iii.HTTPInvocationRef

// MultiValue is a header or query parameter value, string | string[] in the Node types.
// It decodes from a JSON string (one element) or an array of strings; the http worker
// currently sends one string per key. A one-element MultiValue encodes back as a JSON
// string, nil as null and any other length as an array.
type MultiValue []string

// First returns the first value, or "" when there is none.
func (v MultiValue) First() string {
	if len(v) == 0 {
		return ""
	}
	return v[0]
}

// UnmarshalJSON decodes a JSON string, an array of strings or null.
func (v *MultiValue) UnmarshalJSON(data []byte) error {
	trimmed := bytes.TrimSpace(data)
	switch {
	case bytes.Equal(trimmed, []byte("null")):
		*v = nil
		return nil
	case len(trimmed) > 0 && trimmed[0] == '"':
		var s string
		if err := json.Unmarshal(trimmed, &s); err != nil {
			return err
		}
		*v = MultiValue{s}
		return nil
	}
	var values []string
	if err := json.Unmarshal(trimmed, &values); err != nil {
		return fmt.Errorf("iii/helpers/http: a header or query value must be a string or an array of strings: %w", err)
	}
	*v = values
	return nil
}

// MarshalJSON encodes a one-element value as a JSON string, nil as null and any other
// length as an array of strings.
func (v MultiValue) MarshalJSON() ([]byte, error) {
	switch {
	case v == nil:
		return []byte("null"), nil
	case len(v) == 1:
		return json.Marshal(v[0])
	default:
		return json.Marshal([]string(v))
	}
}

// Request is the buffered HTTP request the target of an http::on-request observer
// receives, Node
// HttpRequest<TBody> and Rust HttpRequest<T>. T is the type of the parsed body; use
// json.RawMessage to keep it raw. Decode the handler payload into it, or register the
// function with [iii.RegisterFunctionTyped].
type Request[T any] struct {
	// PathParams holds the path parameters extracted from the matched route.
	PathParams map[string]string `json:"path_params"`
	// QueryParams holds the query string parameters of the request URL.
	QueryParams map[string]MultiValue `json:"query_params"`
	// Body is the parsed request body.
	Body T `json:"body"`
	// Headers holds the request headers.
	Headers map[string]MultiValue `json:"headers"`
	// Path is the route template that matched (for example /users/:id), as in the Rust
	// HttpRequest. The Node type does not declare it; the http worker sends it.
	Path string `json:"path,omitempty"`
	// Method is the HTTP method of the request (for example GET or POST).
	Method string `json:"method"`
	// RequestBody is the raw request body: read it, or pass it on unread. nil when the
	// invocation carried none.
	RequestBody *iii.InputStream `json:"request_body,omitempty"`
}

// Response is a buffered HTTP response returned from the target of an http::on-request
// observer, Node HttpResponse and Rust HttpResponse<T>. T is the type of the body: the http worker
// sends a string Body verbatim when Headers sets a content-type, and encodes anything
// else as JSON (a []byte encodes as a base64 string, so stream binary bodies through a
// [StreamResponse]). A [Wrap] handler returns a Response to answer without streaming.
type Response[T any] struct {
	// StatusCode is the HTTP status code.
	StatusCode int `json:"status_code"`
	// Headers holds the response headers.
	Headers map[string]string `json:"headers,omitempty"`
	// Body is the response body; an omitted body is answered as {}.
	Body T `json:"body,omitempty"`
}

// InternalRequest is the payload the http worker sends to the target of an
// http::on-request observer, Node InternalHttpRequest: the parsed request plus its two
// streams. [Wrap] decodes it; decode it by hand to use the streams directly.
type InternalRequest struct {
	// PathParams holds the path parameters extracted from the matched route.
	PathParams map[string]string `json:"path_params"`
	// QueryParams holds the query string parameters of the request URL.
	QueryParams map[string]MultiValue `json:"query_params"`
	// Body is the parsed request body, as raw JSON.
	Body json.RawMessage `json:"body"`
	// Headers holds the request headers.
	Headers map[string]MultiValue `json:"headers"`
	// Path is the route template that matched. The Node type does not declare it; the
	// http worker sends it.
	Path string `json:"path,omitempty"`
	// Method is the HTTP method of the request.
	Method string `json:"method"`
	// Response is the response stream: control messages and the body.
	Response *iii.OutputStream `json:"response"`
	// RequestBody is the raw request body.
	RequestBody *iii.InputStream `json:"request_body"`
}
