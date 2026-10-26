package observability

import (
	"context"
	"errors"
	"net"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/codes"
	sdkmetric "go.opentelemetry.io/otel/sdk/metric"
	"go.opentelemetry.io/otel/sdk/metric/metricdata"
	sdktrace "go.opentelemetry.io/otel/sdk/trace"
	"go.opentelemetry.io/otel/sdk/trace/tracetest"
	"go.opentelemetry.io/otel/trace"
)

// No client runs telemetry in this package's tests, so the helpers use the global
// tracer provider: a recording one here.
func recordSpans(t *testing.T) *tracetest.SpanRecorder {
	t.Helper()
	rec := tracetest.NewSpanRecorder()
	prev := otel.GetTracerProvider()
	otel.SetTracerProvider(sdktrace.NewTracerProvider(sdktrace.WithSpanProcessor(rec)))
	t.Cleanup(func() { otel.SetTracerProvider(prev) })
	return rec
}

func attrs(s sdktrace.ReadOnlySpan) map[attribute.Key]attribute.Value {
	out := map[attribute.Key]attribute.Value{}
	for _, kv := range s.Attributes() {
		out[kv.Key] = kv.Value
	}
	return out
}

// NewHTTPClient: a CLIENT span named "METHOD /path" with the attributes of Node's
// fetch instrumentation, and the W3C headers injected into the request.
func TestNewHTTPClientTracesRequests(t *testing.T) {
	rec := recordSpans(t)
	var gotTraceparent, gotBaggage string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotTraceparent, gotBaggage = r.Header.Get("traceparent"), r.Header.Get("baggage")
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusCreated)
		_, _ = w.Write([]byte(`{"ok":true}`))
	}))
	defer srv.Close()

	ctx, parent := otel.Tracer("test").Start(context.Background(), "handler")
	ctx, _ = SetBaggageEntry(ctx, "tenant", "acme")
	req, _ := http.NewRequestWithContext(ctx, http.MethodPost, srv.URL+"/api/items?a=1", strings.NewReader(`{"x":1}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer secret")
	resp, err := NewHTTPClient(nil).Do(req)
	if err != nil {
		t.Fatal(err)
	}
	_ = resp.Body.Close()
	parent.End()

	spans := rec.Ended()
	if len(spans) != 2 {
		t.Fatalf("%d spans", len(spans))
	}
	span := spans[0]
	if span.Name() != "POST /api/items" || span.SpanKind() != trace.SpanKindClient || span.Parent().SpanID() != parent.SpanContext().SpanID() {
		t.Errorf("span %q kind %v parent %v", span.Name(), span.SpanKind(), span.Parent().SpanID())
	}
	port := srv.Listener.Addr().(*net.TCPAddr).Port
	want := map[attribute.Key]attribute.Value{
		"http.request.method":               attribute.StringValue("POST"),
		"url.full":                          attribute.StringValue(srv.URL + "/api/items?a=1"),
		"server.address":                    attribute.StringValue("127.0.0.1"),
		"server.port":                       attribute.IntValue(port),
		"url.scheme":                        attribute.StringValue("http"),
		"network.protocol.name":             attribute.StringValue("http"),
		"url.path":                          attribute.StringValue("/api/items"),
		"url.query":                         attribute.StringValue("a=1"),
		"http.request.header.content-type":  attribute.StringValue("application/json"),
		"http.request.body.size":            attribute.Int64Value(7),
		"http.response.status_code":         attribute.IntValue(201),
		"http.response.header.content-type": attribute.StringValue("application/json"),
		"http.response.body.size":           attribute.Int64Value(11),
	}
	got := attrs(span)
	for k, v := range want {
		if got[k] != v {
			t.Errorf("%s = %v, want %v", k, got[k].Emit(), v.Emit())
		}
	}
	if _, leaked := got["http.request.header.authorization"]; leaked {
		t.Error("authorization header recorded")
	}
	if span.Status().Code != codes.Ok {
		t.Errorf("status %+v", span.Status())
	}
	if want := "00-" + span.SpanContext().TraceID().String() + "-" + span.SpanContext().SpanID().String() + "-01"; gotTraceparent != want || gotBaggage != "tenant=acme" {
		t.Errorf("server saw traceparent %q (want %q) baggage %q", gotTraceparent, want, gotBaggage)
	}
}

func TestNewTransportErrorStatus(t *testing.T) {
	rec := recordSpans(t)
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) { w.WriteHeader(http.StatusBadGateway) }))
	defer srv.Close()
	client := &http.Client{Transport: NewTransport(nil)}
	resp, err := client.Get(srv.URL)
	if err != nil {
		t.Fatal(err)
	}
	_ = resp.Body.Close()
	span := rec.Ended()[0]
	if span.Name() != "GET /" && span.Name() != "GET" {
		t.Errorf("name %q", span.Name())
	}
	if span.Status().Code != codes.Error || span.Status().Description != "502" || attrs(span)["error.type"] != attribute.StringValue("502") {
		t.Errorf("status %+v error.type %v", span.Status(), attrs(span)["error.type"])
	}

	l, _ := net.Listen("tcp", "127.0.0.1:0")
	addr := l.Addr().String()
	_ = l.Close()
	if _, err := client.Get("http://" + addr + "/down"); err == nil {
		t.Fatal("request to a closed port succeeded")
	}
	span = rec.Ended()[1]
	if span.Status().Code != codes.Error || attrs(span)["error.type"].AsString() == "" || len(span.Events()) != 1 || span.Events()[0].Name != "exception" {
		t.Errorf("network error span status %+v attrs %v events %v", span.Status(), attrs(span), span.Events())
	}
}

// OTEL_FETCH_IGNORE_URLS: matching requests get no span and no injected headers.
func TestNewTransportIgnoredURLs(t *testing.T) {
	rec := recordSpans(t)
	t.Setenv("OTEL_FETCH_IGNORE_URLS", " /health , /metrics")
	var tp string
	srv := httptest.NewServer(http.HandlerFunc(func(_ http.ResponseWriter, r *http.Request) { tp = r.Header.Get("traceparent") }))
	defer srv.Close()
	ctx, parent := otel.Tracer("test").Start(context.Background(), "handler")
	req, _ := http.NewRequestWithContext(ctx, http.MethodGet, srv.URL+"/health", nil)
	resp, err := NewHTTPClient(nil).Do(req)
	if err != nil {
		t.Fatal(err)
	}
	_ = resp.Body.Close()
	parent.End()
	if len(rec.Ended()) != 1 || tp != "" {
		t.Errorf("ignored URL traced: %d spans, traceparent %q", len(rec.Ended()), tp)
	}
}

func TestNewHTTPClientCopiesBase(t *testing.T) {
	base := &http.Client{Timeout: 3 * time.Second}
	c := NewHTTPClient(base)
	if c == base || base.Transport != nil || c.Timeout != 3*time.Second {
		t.Errorf("base modified or settings lost: %+v %+v", base, c)
	}
	if _, ok := c.Transport.(http.RoundTripper); !ok {
		t.Error("no transport")
	}
}

func TestWithSpan(t *testing.T) {
	rec := recordSpans(t)
	v, err := WithSpan(context.Background(), "ok", SpanOptions{}, func(ctx context.Context, span Span) (int, error) {
		if !CurrentSpanIsRecording(ctx) || CurrentTraceID(ctx) != span.SpanContext().TraceID().String() {
			t.Error("ctx does not carry the span")
		}
		SetCurrentSpanAttribute(ctx, "n", 1)
		RecordSpanEvent(ctx, "step", map[string]any{"i": 2})
		return 42, nil
	})
	if v != 42 || err != nil {
		t.Fatalf("WithSpan = %v, %v", v, err)
	}
	boom := errors.New("boom")
	if _, err := WithSpan(context.Background(), "fails", SpanOptions{Kind: trace.SpanKindServer}, func(context.Context, Span) (string, error) { return "", boom }); !errors.Is(err, boom) {
		t.Fatalf("error not returned: %v", err)
	}
	func() {
		defer func() {
			if recover() == nil {
				t.Error("panic swallowed")
			}
		}()
		_, _ = WithSpan(context.Background(), "panics", SpanOptions{}, func(context.Context, Span) (int, error) { panic("bad") })
	}()
	_, _ = WithSpan(context.Background(), "child", SpanOptions{Traceparent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"}, func(context.Context, Span) (int, error) { return 0, nil })

	spans := rec.Ended()
	if len(spans) != 4 {
		t.Fatalf("%d spans", len(spans))
	}
	if s := spans[0]; s.SpanKind() != trace.SpanKindInternal || s.Status().Code != codes.Ok || attrs(s)["n"] != attribute.IntValue(1) || s.Events()[0].Name != "step" {
		t.Errorf("ok span %v %+v %v %v", s.SpanKind(), s.Status(), s.Attributes(), s.Events())
	}
	if s := spans[1]; s.SpanKind() != trace.SpanKindServer || s.Status().Code != codes.Error || s.Status().Description != "boom" || s.Events()[0].Name != "exception" {
		t.Errorf("failing span %v %+v %v", s.SpanKind(), s.Status(), s.Events())
	}
	if s := spans[2]; s.Status().Code != codes.Error {
		t.Errorf("panicking span %+v", s.Status())
	}
	if s := spans[3]; s.SpanContext().TraceID().String() != "4bf92f3577b34da6a3ce929d0e0e4736" || s.Parent().SpanID().String() != "00f067aa0ba902b7" {
		t.Errorf("traceparent option ignored: %v parent %v", s.SpanContext().TraceID(), s.Parent().SpanID())
	}
}

func TestContextHelpers(t *testing.T) {
	const tp = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
	ctx := ExtractContext(context.Background(), tp, "a=1")
	if InjectTraceparent(ctx) != tp || InjectBaggage(ctx) != "a=1" {
		t.Errorf("round trip %q %q", InjectTraceparent(ctx), InjectBaggage(ctx))
	}
	if CurrentSpanID(ExtractTraceparent(context.Background(), tp)) != "00f067aa0ba902b7" {
		t.Error("ExtractTraceparent")
	}
	if v, ok := GetBaggageEntry(ExtractBaggage(context.Background(), "k=v"), "k"); !ok || v != "v" {
		t.Error("ExtractBaggage")
	}
	ctx, err := SetBaggageEntry(context.Background(), "x", "1")
	if err != nil || GetAllBaggage(ctx)["x"] != "1" || len(GetAllBaggage(RemoveBaggageEntry(ctx, "x"))) != 0 {
		t.Errorf("baggage helpers: %v %v", err, GetAllBaggage(ctx))
	}
	if InjectTraceparent(context.Background()) != "" || CurrentTraceID(context.Background()) != "" {
		t.Error("values without a span")
	}
	SetCurrentSpanError(context.Background(), "no span: no-op")
}

func TestPayloadHelpers(t *testing.T) {
	if s, truncated := RedactAndTruncate(map[string]any{"password": "p", "user": "u"}, 0); s != `{"password":"[REDACTED]","user":"u"}` || truncated {
		t.Errorf("RedactAndTruncate = %s %v", s, truncated)
	}
	if m := Redact(map[string]any{"secret": 1}).(map[string]any); m["secret"] != RedactedPlaceholder {
		t.Errorf("Redact = %v", m)
	}
	t.Setenv("III_TRACE_PAYLOAD_MAX_BYTES", "64")
	if ResolveMaxBytesFromEnv() != 64 {
		t.Error("ResolveMaxBytesFromEnv")
	}
	if SafeStringify(make(chan int)) != "[unserializable]" {
		t.Error("SafeStringify")
	}
}

// Without running client telemetry the Logger falls back to the standard logger, and
// Flush and GetLogger report nothing to do.
func TestNoTelemetryFallbacks(t *testing.T) {
	Logger{ServiceName: "svc"}.Warn(context.Background(), "fallback", map[string]int{"a": 1})
	var l Logger
	l.Debug(context.Background(), "fallback", nil)
	l.Error(context.Background(), "fallback", nil)
	if GetLogger() != nil {
		t.Error("GetLogger without telemetry")
	}
	if err := Flush(context.Background()); err != nil {
		t.Error(err)
	}
}

func TestWorkerMetricsAndGauges(t *testing.T) {
	m := NewWorkerMetricsCollector().Collect()
	if m.Runtime != "go" || m.MemoryHeapUsed == nil || m.UptimeSeconds == nil {
		t.Errorf("collect %+v", m)
	}
	reader := sdkmetric.NewManualReader()
	mp := sdkmetric.NewMeterProvider(sdkmetric.WithReader(reader))
	g, err := RegisterWorkerGauges(mp.Meter("svc"), WorkerGaugesOptions{WorkerID: "w"})
	if err != nil {
		t.Fatal(err)
	}
	var rm metricdata.ResourceMetrics
	_ = reader.Collect(context.Background(), &rm)
	found := false
	for _, metric := range rm.ScopeMetrics[0].Metrics {
		found = found || metric.Name == "iii.worker.uptime_seconds"
	}
	if !found {
		t.Error("uptime gauge missing")
	}
	if err := g.Stop(); err != nil {
		t.Error(err)
	}
}

func TestBaggageSpanProcessor(t *testing.T) {
	rec := tracetest.NewSpanRecorder()
	tp := sdktrace.NewTracerProvider(sdktrace.WithSpanProcessor(BaggageSpanProcessor{}), sdktrace.WithSpanProcessor(rec))
	ctx, _ := SetBaggageEntry(context.Background(), "iii.session.id", "s-1")
	_, span := tp.Tracer("t").Start(ctx, "s")
	span.End()
	if got := attrs(rec.Ended()[0])["iii.session.id"]; got != attribute.StringValue("s-1") {
		t.Errorf("baggage attribute %v", got)
	}
}

// Review round 1, finding 3: url.full and url.query carry neither the URL's credentials
// nor the values of the semconv sensitive query keys, while the request itself is sent
// unchanged.
func TestNewTransportRedactsCredentials(t *testing.T) {
	rec := recordSpans(t)
	var gotQuery, gotUser, gotPass string
	srv := httptest.NewServer(http.HandlerFunc(func(_ http.ResponseWriter, r *http.Request) {
		gotQuery = r.URL.RawQuery
		gotUser, gotPass, _ = r.BasicAuth()
	}))
	defer srv.Close()

	const query = "AWSAccessKeyId=AKIAVALUE1&Signature=sigvalue1&sig=sigvalue2&X-Goog-Signature=googvalue1&keep=1"
	host := strings.TrimPrefix(srv.URL, "http://")
	req, _ := http.NewRequest(http.MethodGet, "http://alice:s3cr3t-pw@"+host+"/files/report?"+query, nil)
	resp, err := NewHTTPClient(nil).Do(req)
	if err != nil {
		t.Fatal(err)
	}
	_ = resp.Body.Close()

	if gotQuery != query || gotUser != "alice" || gotPass != "s3cr3t-pw" {
		t.Errorf("the request was modified: query %q user %q password %q", gotQuery, gotUser, gotPass)
	}
	span := rec.Ended()[0]
	got := attrs(span)
	wantQuery := "AWSAccessKeyId=REDACTED&Signature=REDACTED&sig=REDACTED&X-Goog-Signature=REDACTED&keep=1"
	if full := got["url.full"].AsString(); full != "http://REDACTED:REDACTED@"+host+"/files/report?"+wantQuery {
		t.Errorf("url.full = %q", full)
	}
	if q := got["url.query"].AsString(); q != wantQuery {
		t.Errorf("url.query = %q", q)
	}
	for _, secret := range []string{"alice", "s3cr3t-pw", "AKIAVALUE1", "sigvalue1", "sigvalue2", "googvalue1"} {
		if strings.Contains(span.Name(), secret) {
			t.Errorf("span name %q leaks %q", span.Name(), secret)
		}
		for k, v := range got {
			if strings.Contains(v.Emit(), secret) {
				t.Errorf("attribute %s = %q leaks %q", k, v.Emit(), secret)
			}
		}
	}
}
