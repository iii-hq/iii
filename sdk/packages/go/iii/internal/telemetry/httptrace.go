package telemetry

import (
	"fmt"
	"net/http"
	"os"
	"strconv"
	"strings"

	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/codes"
	"go.opentelemetry.io/otel/propagation"
	"go.opentelemetry.io/otel/trace"
)

// Headers recorded as span attributes (Node SAFE_REQUEST_HEADERS / SAFE_RESPONSE_HEADERS).
var (
	safeRequestHeaders  = []string{"content-type", "accept"}
	safeResponseHeaders = []string{"content-type"}
)

// Transport is an http.RoundTripper that runs each request inside an OpenTelemetry
// CLIENT span, the Go counterpart of the Node SDK's patchGlobalFetch and
// executeTracedRequest: span name "METHOD /path", HTTP semantic-convention attributes,
// W3C traceparent/baggage injected into the request, ERROR status for network errors
// and responses with status >= 400.
type Transport struct {
	Base http.RoundTripper
}

// fetchIgnored reports whether url matches one of the comma-separated substrings in
// OTEL_FETCH_IGNORE_URLS: such requests run without a span or injected headers.
func fetchIgnored(url string) bool {
	for _, p := range strings.Split(os.Getenv("OTEL_FETCH_IGNORE_URLS"), ",") {
		if p = strings.TrimSpace(p); p != "" && strings.Contains(url, p) {
			return true
		}
	}
	return false
}

// fetchInstrumentationEnabled is false when the running client disabled it.
func fetchInstrumentationEnabled() bool {
	if s := Active(); s != nil {
		return s.cfg.FetchInstrumentationEnabled
	}
	return true
}

// RoundTrip implements http.RoundTripper.
func (t *Transport) RoundTrip(req *http.Request) (*http.Response, error) {
	base := t.Base
	if base == nil {
		base = http.DefaultTransport
	}
	rawURL := req.URL.String()
	if !fetchInstrumentationEnabled() || fetchIgnored(rawURL) {
		return base.RoundTrip(req)
	}

	method := strings.ToUpper(req.Method)
	if method == "" {
		method = http.MethodGet
	}
	attrs := []attribute.KeyValue{
		attribute.String("http.request.method", method),
		attribute.String("url.full", rawURL),
	}
	if host := req.URL.Hostname(); host != "" {
		attrs = append(attrs, attribute.String("server.address", host))
	}
	if req.URL.Scheme != "" {
		attrs = append(attrs, attribute.String("url.scheme", req.URL.Scheme), attribute.String("network.protocol.name", "http"))
	}
	path := req.URL.EscapedPath()
	if path != "" {
		attrs = append(attrs, attribute.String("url.path", path))
	}
	if port, err := strconv.Atoi(req.URL.Port()); err == nil && port > 0 {
		attrs = append(attrs, attribute.Int("server.port", port))
	}
	if req.URL.RawQuery != "" {
		attrs = append(attrs, attribute.String("url.query", req.URL.RawQuery))
	}
	name := method
	if path != "" {
		name = method + " " + path
	}

	ctx, span := Tracer().Start(req.Context(), name, trace.WithSpanKind(trace.SpanKindClient), trace.WithAttributes(attrs...))
	defer span.End()

	// A RoundTripper must not modify the caller's request.
	req = req.Clone(ctx)
	if req.Header == nil {
		req.Header = http.Header{}
	}
	Propagator.Inject(ctx, propagation.HeaderCarrier(req.Header))
	for _, h := range safeRequestHeaders {
		if v := req.Header.Get(h); v != "" {
			span.SetAttributes(attribute.String("http.request.header."+h, v))
		}
	}
	if req.ContentLength > 0 {
		span.SetAttributes(attribute.Int64("http.request.body.size", req.ContentLength))
	}

	resp, err := base.RoundTrip(req)
	if err != nil {
		span.SetAttributes(attribute.String("error.type", fmt.Sprintf("%T", err)))
		span.SetStatus(codes.Error, err.Error())
		span.RecordError(err)
		return nil, err
	}
	span.SetAttributes(attribute.Int("http.response.status_code", resp.StatusCode))
	if cl, perr := strconv.ParseInt(resp.Header.Get("Content-Length"), 10, 64); perr == nil {
		span.SetAttributes(attribute.Int64("http.response.body.size", cl))
	} else if resp.ContentLength >= 0 {
		span.SetAttributes(attribute.Int64("http.response.body.size", resp.ContentLength))
	}
	for _, h := range safeResponseHeaders {
		if v := resp.Header.Get(h); v != "" {
			span.SetAttributes(attribute.String("http.response.header."+h, v))
		}
	}
	if resp.StatusCode >= 400 {
		span.SetAttributes(attribute.String("error.type", strconv.Itoa(resp.StatusCode)))
		span.SetStatus(codes.Error, strconv.Itoa(resp.StatusCode))
	} else {
		span.SetStatus(codes.Ok, "")
	}
	return resp, nil
}
