package observability

import (
	"net/http"

	"github.com/iii-hq/iii/sdk/packages/go/iii/internal/telemetry"
)

// NewTransport wraps base (http.DefaultTransport when nil) so every request runs inside
// an OpenTelemetry CLIENT span, the Go counterpart of the Node SDK's executeTracedRequest
// and patchGlobalFetch. The span is named "METHOD /path" and carries the HTTP semantic
// convention attributes (http.request.method, url.full, server.address, server.port,
// url.scheme, url.path, url.query, http.response.status_code, the content-type and
// accept headers and the body sizes); the W3C traceparent and baggage of the span are
// injected into the request; a network error or a status >= 400 sets an ERROR status
// and error.type. url.full and url.query carry no credentials, as the OpenTelemetry
// HTTP semantic conventions require: userinfo becomes REDACTED:REDACTED and the values
// of the AWSAccessKeyId, Signature, sig and X-Goog-Signature query parameters become
// REDACTED. The request itself is sent unchanged.
//
// The parent span comes from the request's context (http.NewRequestWithContext with a
// handler's ctx). URLs containing one of the comma-separated substrings of
// OTEL_FETCH_IGNORE_URLS, or every request when the running client's
// OtelConfig.FetchInstrumentationEnabled is false, pass through untraced.
func NewTransport(base http.RoundTripper) http.RoundTripper {
	return &telemetry.Transport{Base: base}
}

// NewHTTPClient returns a copy of base (a zero http.Client when nil) whose transport is
// wrapped with [NewTransport]. base itself is not modified.
//
//	client := observability.NewHTTPClient(nil)
//	req, _ := http.NewRequestWithContext(ctx, http.MethodGet, "https://example.com/api", nil)
//	resp, err := client.Do(req)
func NewHTTPClient(base *http.Client) *http.Client {
	if base == nil {
		return &http.Client{Transport: NewTransport(nil)}
	}
	out := *base
	out.Transport = NewTransport(base.Transport)
	return &out
}
