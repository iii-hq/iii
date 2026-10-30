package iii

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"os"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/coder/websocket"
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/baggage"
	"go.opentelemetry.io/otel/trace"

	"github.com/iii-hq/iii/sdk/packages/go/iii/internal/telemetry"
	"github.com/iii-hq/iii/sdk/packages/go/iii/observability"
)

// These tests run a client against one fake engine serving the worker protocol on "/"
// (the mock engine) and the telemetry endpoint on "/otel" (otelCollector), and check
// the frames the client exports against the Node SDK's format.

const (
	testTraceID     = "4bf92f3577b34da6a3ce929d0e0e4736"
	testSpanID      = "00f067aa0ba902b7"
	testTraceparent = "00-" + testTraceID + "-" + testSpanID + "-01"
)

// otelCollector is the engine's /otel endpoint: it records handshakes and binary frames.
type otelCollector struct {
	mu      sync.Mutex
	cond    *sync.Cond
	frames  [][]byte
	conns   int
	closed  int
	headers []http.Header
}

func (oc *otelCollector) handle(w http.ResponseWriter, r *http.Request) {
	conn, err := websocket.Accept(w, r, nil)
	if err != nil {
		return
	}
	conn.SetReadLimit(64 << 20)
	oc.mu.Lock()
	oc.conns++
	oc.headers = append(oc.headers, r.Header.Clone())
	oc.cond.Broadcast()
	oc.mu.Unlock()
	for {
		typ, data, err := conn.Read(context.Background())
		if err != nil {
			oc.mu.Lock()
			oc.closed++
			oc.cond.Broadcast()
			oc.mu.Unlock()
			return
		}
		if typ == websocket.MessageBinary {
			oc.mu.Lock()
			oc.frames = append(oc.frames, data)
			oc.cond.Broadcast()
			oc.mu.Unlock()
		}
	}
}

func (oc *otelCollector) waitFor(t *testing.T, what string, cond func() bool) {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	stop := make(chan struct{})
	defer close(stop)
	go func() {
		select {
		case <-time.After(5 * time.Second):
			oc.mu.Lock()
			oc.cond.Broadcast()
			oc.mu.Unlock()
		case <-stop:
		}
	}()
	oc.mu.Lock()
	defer oc.mu.Unlock()
	for !cond() {
		if time.Now().After(deadline) {
			t.Fatalf("timed out waiting for %s", what)
		}
		oc.cond.Wait()
	}
}

// payloads returns the JSON documents of the frames with the given signal prefix.
func (oc *otelCollector) payloads(prefix string) [][]byte {
	oc.mu.Lock()
	defer oc.mu.Unlock()
	var out [][]byte
	for _, f := range oc.frames {
		if bytes.HasPrefix(f, []byte(prefix)) {
			out = append(out, f[len(prefix):])
		}
	}
	return out
}

func (oc *otelCollector) connections() int {
	oc.mu.Lock()
	defer oc.mu.Unlock()
	return oc.conns
}

type otelEngine struct {
	m   *mockEngine
	oc  *otelCollector
	url string
}

func newOtelEngine(t *testing.T, worker http.HandlerFunc) *otelEngine {
	t.Helper()
	e := &otelEngine{oc: &otelCollector{}}
	e.oc.cond = sync.NewCond(&e.oc.mu)
	if worker == nil {
		e.m = newMockEngine(t)
		worker = e.m.handle
	}
	mux := http.NewServeMux()
	mux.HandleFunc("/otel", e.oc.handle)
	mux.HandleFunc("/", worker)
	srv := httptest.NewServer(mux)
	t.Cleanup(srv.Close)
	e.url = "ws" + strings.TrimPrefix(srv.URL, "http")
	return e
}

func (e *otelEngine) connect(t *testing.T, opts ...Option) *Client {
	t.Helper()
	c := New(e.url, opts...)
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err := c.Connect(ctx); err != nil {
		t.Fatalf("Connect: %v", err)
	}
	t.Cleanup(func() { _ = c.Close() })
	return c
}

// otelOn enables telemetry explicitly, over the OTEL_ENABLED=false of TestMain.
func otelOn(cfg OtelConfig) Option {
	on := true
	cfg.Enabled = &on
	if cfg.SpansFlushInterval == 0 {
		cfg.SpansFlushInterval = 5 * time.Millisecond
	}
	return WithOtel(cfg)
}

// sendInvoke makes the mock engine invoke fn once it is registered.
func (e *otelEngine) invokeOnRegister(t *testing.T, fn string, data string, traceparent, bag string) {
	t.Helper()
	id := mustUUID(t, "33333333-3333-3333-3333-333333333333")
	e.m.onReceive = func(conn *websocket.Conn, msg map[string]json.RawMessage) {
		if messageType(msg) != string(MsgRegisterFunction) || messageID(msg) != fn {
			return
		}
		inv := &InvokeFunctionMessage{InvocationID: &id, FunctionID: fn, Data: json.RawMessage(data)}
		if traceparent != "" {
			inv.Traceparent = &traceparent
		}
		if bag != "" {
			inv.Baggage = &bag
		}
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		_ = e.m.send(ctx, conn, inv)
	}
}

func (e *otelEngine) waitInvoke(t *testing.T, fn string) map[string]json.RawMessage {
	t.Helper()
	match := func(msgs []map[string]json.RawMessage) map[string]json.RawMessage {
		for _, m := range msgs {
			if messageType(m) == string(MsgInvokeFunction) && stringField(m, "function_id") == fn {
				return m
			}
		}
		return nil
	}
	msgs := e.m.waitFor(func(msgs []map[string]json.RawMessage) bool { return match(msgs) != nil }, 3*time.Second)
	m := match(msgs)
	if m == nil {
		t.Fatalf("no invokefunction for %s", fn)
	}
	return m
}

func (e *otelEngine) waitResult(t *testing.T) map[string]json.RawMessage {
	t.Helper()
	msgs := e.m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return firstOfType(msgs, string(MsgInvocationResult)) != nil
	}, 3*time.Second)
	res := firstOfType(msgs, string(MsgInvocationResult))
	if res == nil {
		t.Fatal("no invocationresult")
	}
	return res
}

// OTLP/JSON documents, decoded loosely (field names match case-insensitively).
type otlpKV struct {
	Key   string
	Value map[string]any
}

type otlpSpan struct {
	TraceID           string
	SpanID            string
	ParentSpanID      string
	Name              string
	Kind              int
	StartTimeUnixNano string
	EndTimeUnixNano   string
	Attributes        []otlpKV
	Events            []struct {
		Name         string
		TimeUnixNano string
		Attributes   []otlpKV
	}
	Status struct {
		Code    int
		Message string
	}
	Flags int
}

type otlpTraces struct {
	ResourceSpans []struct {
		Resource   struct{ Attributes []otlpKV }
		ScopeSpans []struct {
			Scope struct{ Name string }
			Spans []otlpSpan
		}
	}
}

func attr(kvs []otlpKV, key string) any {
	for _, kv := range kvs {
		if kv.Key == key {
			for _, v := range kv.Value {
				return v
			}
		}
	}
	return nil
}

// findSpan decodes the span frames and returns the span named name with its resource
// attributes and scope.
func findSpan(oc *otelCollector, name string) (otlpSpan, []otlpKV, string, bool) {
	for _, p := range oc.payloads(telemetry.PrefixTraces) {
		var req otlpTraces
		if json.Unmarshal(p, &req) != nil {
			continue
		}
		for _, rs := range req.ResourceSpans {
			for _, ss := range rs.ScopeSpans {
				for _, s := range ss.Spans {
					if s.Name == name {
						return s, rs.Resource.Attributes, ss.Scope.Name, true
					}
				}
			}
		}
	}
	return otlpSpan{}, nil, "", false
}

func waitSpan(t *testing.T, oc *otelCollector, name string) (otlpSpan, []otlpKV, string) {
	t.Helper()
	var span otlpSpan
	var res []otlpKV
	var scope string
	deadline := time.Now().Add(5 * time.Second)
	for {
		var ok bool
		if span, res, scope, ok = findSpan(oc, name); ok {
			return span, res, scope
		}
		if time.Now().After(deadline) {
			t.Fatalf("span %q never exported; frames: %q", name, oc.payloads(telemetry.PrefixTraces))
		}
		time.Sleep(5 * time.Millisecond)
	}
}

func spanEvent(s otlpSpan, name string) ([]otlpKV, bool) {
	for _, e := range s.Events {
		if e.Name == name {
			return e.Attributes, true
		}
	}
	return nil, false
}

func tracedContext(t *testing.T) context.Context {
	t.Helper()
	tid, _ := trace.TraceIDFromHex(testTraceID)
	sid, _ := trace.SpanIDFromHex(testSpanID)
	ctx := trace.ContextWithSpanContext(context.Background(), trace.NewSpanContext(trace.SpanContextConfig{TraceID: tid, SpanID: sid, TraceFlags: trace.FlagsSampled}))
	m, _ := baggage.NewMemberRaw("tenant", "acme")
	b, _ := baggage.New(m)
	return baggage.ContextWithBaggage(ctx, b)
}

// B2: Trigger injects the traceparent and baggage of ctx into the outbound invoke frame,
// with telemetry export disabled (TestMain's OTEL_ENABLED=false), and omits them when
// ctx carries none.
func TestOtelTriggerInjectsTraceContext(t *testing.T) {
	e := newOtelEngine(t, nil)
	c := e.connect(t)
	if _, err := c.Trigger(tracedContext(t), TriggerRequest{FunctionID: "remote::traced", Action: VoidAction()}); err != nil {
		t.Fatal(err)
	}
	if _, err := c.Trigger(context.Background(), TriggerRequest{FunctionID: "remote::plain", Action: VoidAction()}); err != nil {
		t.Fatal(err)
	}
	traced := e.waitInvoke(t, "remote::traced")
	if stringField(traced, "traceparent") != testTraceparent || stringField(traced, "baggage") != "tenant=acme" {
		t.Errorf("traced frame traceparent %q baggage %q", stringField(traced, "traceparent"), stringField(traced, "baggage"))
	}
	plain := e.waitInvoke(t, "remote::plain")
	if _, ok := plain["traceparent"]; ok {
		t.Errorf("frame without trace context carries traceparent %s", plain["traceparent"])
	}
	if _, ok := plain["baggage"]; ok {
		t.Errorf("frame without baggage carries baggage %s", plain["baggage"])
	}
	if n := e.oc.connections(); n != 0 {
		t.Errorf("telemetry disabled but %d /otel connections", n)
	}
}

// B2: an inbound invocation's traceparent and baggage are extracted into the handler's
// ctx, and the calls the handler makes with it carry them on (telemetry disabled: the
// remote span itself is the parent).
func TestOtelHandlerReceivesCallerTraceContext(t *testing.T) {
	e := newOtelEngine(t, nil)
	e.invokeOnRegister(t, "traced::fn", `{}`, testTraceparent, "tenant=acme")
	c := e.connect(t)
	type seen struct {
		sc     trace.SpanContext
		tenant string
	}
	got := make(chan seen, 1)
	_, _ = c.RegisterFunction("traced::fn", func(ctx context.Context, _ json.RawMessage) (any, error) {
		got <- seen{trace.SpanContextFromContext(ctx), baggage.FromContext(ctx).Member("tenant").Value()}
		_, err := c.Trigger(ctx, TriggerRequest{FunctionID: "downstream::fn", Action: VoidAction()})
		return "ok", err
	})
	var s seen
	select {
	case s = <-got:
	case <-time.After(5 * time.Second):
		t.Fatal("the handler was never invoked")
	}
	if s.sc.TraceID().String() != testTraceID || s.sc.SpanID().String() != testSpanID || !s.sc.IsRemote() || s.tenant != "acme" {
		t.Errorf("handler ctx span %v remote %v tenant %q", s.sc, s.sc.IsRemote(), s.tenant)
	}
	down := e.waitInvoke(t, "downstream::fn")
	if stringField(down, "traceparent") != testTraceparent || stringField(down, "baggage") != "tenant=acme" {
		t.Errorf("downstream traceparent %q baggage %q", stringField(down, "traceparent"), stringField(down, "baggage"))
	}
	if res := e.waitResult(t); stringField(res, "traceparent") != testTraceparent || stringField(res, "baggage") != "tenant=acme" {
		t.Errorf("result echoes traceparent %q baggage %q", stringField(res, "traceparent"), stringField(res, "baggage"))
	}
}

// B2/B3: with telemetry on, the handler runs in an "execute <fn>" span parented to the
// caller, exported to /otel in the Node format, and the handler's calls are parented to
// that span.
func TestOtelInvocationSpanExported(t *testing.T) {
	e := newOtelEngine(t, nil)
	e.invokeOnRegister(t, "orders::create", `{"item":"book","password":"hunter2"}`, testTraceparent, "tenant=acme")
	c := e.connect(t,
		otelOn(OtelConfig{ServiceName: "orders-svc", ServiceVersion: "1.0.0"}),
		WithHeaders(http.Header{"X-Test": {"1"}}),
	)
	_, _ = c.RegisterFunction("orders::create", func(ctx context.Context, _ json.RawMessage) (any, error) {
		_, err := c.Trigger(ctx, TriggerRequest{FunctionID: "inventory::reserve", Action: VoidAction()})
		return map[string]any{"ok": true, "token": "t-1"}, err
	})

	span, res, scope := waitSpan(t, e.oc, "execute orders::create")
	if span.TraceID != testTraceID || span.ParentSpanID != testSpanID || span.Kind != 1 || span.Status.Code != 1 || span.Flags != 0x301 {
		t.Errorf("span trace %s parent %s kind %d status %d flags %#x", span.TraceID, span.ParentSpanID, span.Kind, span.Status.Code, span.Flags)
	}
	if span.StartTimeUnixNano == "" || strings.Trim(span.StartTimeUnixNano, "0123456789") != "" {
		t.Errorf("startTimeUnixNano %q is not a decimal string", span.StartTimeUnixNano)
	}
	if attr(span.Attributes, "tenant") != "acme" {
		t.Errorf("baggage not copied onto the span: %v", span.Attributes)
	}
	in, ok := spanEvent(span, "iii.invocation.input")
	if !ok || attr(in, "iii.payload.json") != `{"item":"book","password":"[REDACTED]"}` || attr(in, "iii.payload.truncated") != false {
		t.Errorf("input event %v", in)
	}
	out, ok := spanEvent(span, "iii.invocation.output")
	if !ok || attr(out, "iii.payload.json") != `{"ok":true,"token":"[REDACTED]"}` || attr(out, "iii.payload.ok") != true {
		t.Errorf("output event %v", out)
	}
	if attr(res, "service.name") != "orders-svc" || attr(res, "service.version") != "1.0.0" || attr(res, "service.instance.id") == nil || scope != "orders-svc" {
		t.Errorf("resource %v scope %q", res, scope)
	}

	down := e.waitInvoke(t, "inventory::reserve")
	if want := "00-" + testTraceID + "-" + span.SpanID + "-01"; stringField(down, "traceparent") != want || stringField(down, "baggage") != "tenant=acme" {
		t.Errorf("downstream traceparent %q (want %q) baggage %q", stringField(down, "traceparent"), want, stringField(down, "baggage"))
	}
	e.oc.mu.Lock()
	h := e.oc.headers[0]
	e.oc.mu.Unlock()
	if h.Get("X-Test") != "1" {
		t.Errorf("/otel handshake headers %v lack the client's WithHeaders", h)
	}
}

// A failing handler ends its span with ERROR, an exception event and ok=false output.
func TestOtelInvocationSpanError(t *testing.T) {
	for _, tc := range []struct {
		name, fn, wantType, wantMessage, wantStack string
		err                                        error
	}{
		{name: "invocation error", fn: "fail::typed", err: &InvocationError{Code: "E_BAD", Message: "nope", Stacktrace: "at x"}, wantType: "E_BAD", wantMessage: "nope", wantStack: "at x"},
		{name: "plain error", fn: "fail::plain", err: errors.New("boom"), wantType: "*errors.errorString", wantMessage: "boom"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			e := newOtelEngine(t, nil)
			e.invokeOnRegister(t, tc.fn, `{}`, "", "")
			c := e.connect(t, otelOn(OtelConfig{}))
			_, _ = c.RegisterFunction(tc.fn, func(context.Context, json.RawMessage) (any, error) { return nil, tc.err })
			span, _, _ := waitSpan(t, e.oc, "execute "+tc.fn)
			if span.Status.Code != 2 || span.Status.Message != tc.wantMessage || span.ParentSpanID != "" {
				t.Errorf("status %+v parent %q", span.Status, span.ParentSpanID)
			}
			out, _ := spanEvent(span, "iii.invocation.output")
			if attr(out, "iii.payload.ok") != false || attr(out, "iii.payload.json") != `{"error":"`+tc.wantMessage+`"}` {
				t.Errorf("output event %v", out)
			}
			exc, ok := spanEvent(span, "exception")
			if !ok || attr(exc, "exception.type") != tc.wantType || attr(exc, "exception.message") != tc.wantMessage {
				t.Errorf("exception event %v", exc)
			}
			if tc.wantStack != "" && attr(exc, "exception.stacktrace") != tc.wantStack {
				t.Errorf("exception stacktrace %v", exc)
			}
		})
	}
}

// B3: III_DISABLE_TRACE_PAYLOADS ("1" or "true", any case) drops the payload events.
func TestOtelDisableTracePayloads(t *testing.T) {
	for _, value := range []string{"1", "TRUE"} {
		t.Run(value, func(t *testing.T) {
			t.Setenv("III_DISABLE_TRACE_PAYLOADS", value)
			e := newOtelEngine(t, nil)
			e.invokeOnRegister(t, "quiet::fn", `{"secret":"s"}`, "", "")
			c := e.connect(t, otelOn(OtelConfig{}))
			_, _ = c.RegisterFunction("quiet::fn", func(context.Context, json.RawMessage) (any, error) { return 1, nil })
			span, _, _ := waitSpan(t, e.oc, "execute quiet::fn")
			if len(span.Events) != 0 {
				t.Errorf("payload events recorded: %+v", span.Events)
			}
			if span.Status.Code != 1 {
				t.Errorf("status %+v", span.Status)
			}
		})
	}
}

// V6: Shutdown flushes the telemetry buffered behind long flush intervals within its ctx,
// then stops the exporters, as Close does.
func TestOtelShutdownFlushesAndStops(t *testing.T) {
	e := newOtelEngine(t, nil)
	c := e.connect(t,
		otelOn(OtelConfig{ServiceName: "shutdown-svc", SpansFlushInterval: time.Hour, LogsFlushInterval: time.Hour, MetricsExportInterval: time.Hour, LogsBatchSize: 100}),
		WithMetricsReporting(false),
	)
	e.oc.waitFor(t, "telemetry connection", func() bool { return e.oc.conns == 1 })
	_, span := otel.Tracer("user").Start(context.Background(), "before-shutdown")
	span.End()
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := c.Shutdown(ctx); err != nil {
		t.Fatalf("Shutdown: %v", err)
	}
	if _, _, _, ok := findSpan(e.oc, "before-shutdown"); !ok {
		t.Errorf("span not flushed by Shutdown: %q", e.oc.payloads(telemetry.PrefixTraces))
	}
	e.oc.waitFor(t, "telemetry socket closed", func() bool { return e.oc.closed == 1 })
	if telemetry.Active() != nil || c.telemetrySystem() != nil {
		t.Error("telemetry still running after Shutdown")
	}
}

// B3: Close flushes spans, logs and metrics buffered behind long flush intervals, then
// closes the telemetry socket and stops the exporters.
func TestOtelCloseFlushesAndStops(t *testing.T) {
	e := newOtelEngine(t, nil)
	c := e.connect(t,
		otelOn(OtelConfig{ServiceName: "flush-svc", SpansFlushInterval: time.Hour, LogsFlushInterval: time.Hour, MetricsExportInterval: time.Hour, LogsBatchSize: 100}),
		WithMetricsReporting(false),
	)
	e.oc.waitFor(t, "telemetry connection", func() bool { return e.oc.conns == 1 })

	ctx, span := otel.Tracer("user").Start(context.Background(), "user-work")
	observability.Logger{ServiceName: "flush-svc"}.Info(ctx, "about to close", map[string]any{"n": 1})
	span.End()
	counter, err := otel.Meter("user").Int64Counter("flush.counter")
	if err != nil {
		t.Fatal(err)
	}
	counter.Add(ctx, 3)

	time.Sleep(50 * time.Millisecond)
	if n := len(e.oc.payloads("")); n != 0 {
		t.Fatalf("%d frames exported before Close despite 1h flush intervals", n)
	}
	if err := c.Close(); err != nil {
		t.Fatal(err)
	}

	if _, _, _, ok := findSpan(e.oc, "user-work"); !ok {
		t.Errorf("span not flushed by Close: %q", e.oc.payloads(telemetry.PrefixTraces))
	}
	logs := e.oc.payloads(telemetry.PrefixLogs)
	traceID := span.SpanContext().TraceID().String()
	if len(logs) != 1 || !bytes.Contains(logs[0], []byte(`"body":{"stringValue":"about to close"}`)) ||
		!bytes.Contains(logs[0], []byte(`"severityNumber":9`)) ||
		!bytes.Contains(logs[0], []byte(`"traceId":"`+traceID+`"`)) ||
		!bytes.Contains(logs[0], []byte(`{"key":"trace_id","value":{"stringValue":"`+traceID+`"}}`)) ||
		!bytes.Contains(logs[0], []byte(`{"key":"service.name","value":{"stringValue":"flush-svc"}}`)) ||
		!bytes.Contains(logs[0], []byte(`{"key":"log.data","value":{"kvlistValue":{"values":[{"key":"n","value":{"intValue":1}}]}}}`)) {
		t.Errorf("log not flushed by Close in the Node format: %q", logs)
	}
	metrics := e.oc.payloads(telemetry.PrefixMetrics)
	if len(metrics) == 0 || !bytes.Contains(metrics[0], []byte(`"name":"flush.counter"`)) || !bytes.Contains(metrics[0], []byte(`"aggregationTemporality":2,"isMonotonic":true`)) ||
		!bytes.Contains(metrics[0], []byte(`"asInt":3`)) {
		t.Errorf("metrics not flushed by Close: %q", metrics)
	}
	e.oc.waitFor(t, "telemetry socket closed", func() bool { return e.oc.closed == 1 })
	if telemetry.Active() != nil || observability.GetLogger() != nil || c.telemetrySystem() != nil {
		t.Error("telemetry still running after Close")
	}
	// Exporters are stopped: nothing new reaches the engine.
	_, late := otel.Tracer("user").Start(context.Background(), "after-close")
	late.End()
	time.Sleep(30 * time.Millisecond)
	if _, _, _, ok := findSpan(e.oc, "after-close"); ok {
		t.Error("a span ended after Close was exported")
	}
}

// B1: disabled telemetry, explicitly or through OTEL_ENABLED, produces no telemetry
// traffic at all.
func TestOtelDisabledProducesNoTraffic(t *testing.T) {
	for _, tc := range []struct {
		name string
		opts []Option
		env  string
	}{
		{name: "OtelConfig.Enabled false", opts: []Option{WithOtel(OtelConfig{Enabled: new(bool)})}, env: "unset"},
		{name: "OTEL_ENABLED=false", env: "false"},
		{name: "OTEL_ENABLED=off", env: "off"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			t.Setenv("OTEL_ENABLED", tc.env)
			if tc.env == "unset" {
				_ = os.Unsetenv("OTEL_ENABLED")
			}
			e := newOtelEngine(t, nil)
			e.invokeOnRegister(t, "silent::fn", `{}`, testTraceparent, "")
			c := e.connect(t, tc.opts...)
			_, _ = c.RegisterFunction("silent::fn", func(context.Context, json.RawMessage) (any, error) { return 1, nil })
			e.waitResult(t)
			observability.Logger{}.Info(context.Background(), "to the standard logger", nil)
			_ = c.Close()
			if n := e.oc.connections(); n != 0 {
				t.Errorf("%d /otel connections with telemetry disabled", n)
			}
			if c.telemetrySystem() != nil {
				t.Error("telemetry pipeline started")
			}
		})
	}
}

// B1: like Node registerWorker, telemetry is on by default: without WithOtel and without
// OTEL_ENABLED the client exports to <address>/otel with the default service name.
func TestOtelEnabledByDefault(t *testing.T) {
	t.Setenv("OTEL_ENABLED", "")
	_ = os.Unsetenv("OTEL_ENABLED")
	t.Setenv("OTEL_SERVICE_NAME", "")
	t.Setenv("OTEL_SPANS_FLUSH_INTERVAL_MS", "5")
	e := newOtelEngine(t, nil)
	e.invokeOnRegister(t, "default::fn", `{}`, "", "")
	c := e.connect(t)
	_, _ = c.RegisterFunction("default::fn", func(context.Context, json.RawMessage) (any, error) { return 1, nil })
	_, res, scope := waitSpan(t, e.oc, "execute default::fn")
	if attr(res, "service.name") != "iii-go" || attr(res, "service.version") != "unknown" || scope != "iii-go" {
		t.Errorf("default resource %v scope %q", res, scope)
	}
	if e.oc.connections() != 1 {
		t.Errorf("%d /otel connections", e.oc.connections())
	}
}

// B4: once the engine acknowledges the worker, the worker gauges are exported with its
// id and name; WithMetricsReporting(false) turns them off.
func TestOtelWorkerGauges(t *testing.T) {
	for _, enabled := range []bool{true, false} {
		t.Run(map[bool]string{true: "enabled", false: "disabled"}[enabled], func(t *testing.T) {
			e := newOtelEngine(t, nil)
			e.m.onConnect = func(conn *websocket.Conn) {
				_ = e.m.send(context.Background(), conn, &WorkerRegisteredMessage{WorkerID: "w-otel"})
			}
			e.connect(t, WithName("gauge-worker"), WithMetricsReporting(enabled), otelOn(OtelConfig{MetricsExportInterval: 20 * time.Millisecond}))
			if !enabled {
				time.Sleep(200 * time.Millisecond)
				for _, p := range e.oc.payloads(telemetry.PrefixMetrics) {
					if bytes.Contains(p, []byte("iii.worker.")) {
						t.Fatalf("worker gauges exported with metrics reporting off: %s", p)
					}
				}
				return
			}
			want := [][]byte{
				[]byte(`"name":"iii.worker.memory.heap_used","description":"Worker heap memory used in bytes","unit":"bytes","gauge":{"dataPoints":[{"attributes":[{"key":"worker.id","value":{"stringValue":"w-otel"}},{"key":"worker.name","value":{"stringValue":"gauge-worker"}}]`),
				[]byte(`"name":"iii.worker.uptime_seconds","description":"Worker uptime in seconds","unit":"s"`),
				[]byte(`"name":"iii.worker.event_loop.lag_ms"`),
			}
			deadline := time.Now().Add(5 * time.Second)
			for {
				found := false
				for _, p := range e.oc.payloads(telemetry.PrefixMetrics) {
					all := true
					for _, w := range want {
						all = all && bytes.Contains(p, w)
					}
					found = found || (all && bytes.Contains(p, []byte(`"asDouble":`)))
				}
				if found {
					return
				}
				if time.Now().After(deadline) {
					t.Fatalf("worker gauges never exported: %q", e.oc.payloads(telemetry.PrefixMetrics))
				}
				time.Sleep(10 * time.Millisecond)
			}
		})
	}
}

// A local (in-process) call gets its own "execute" span, parented to the caller's span.
func TestOtelLocalDispatchSpanParent(t *testing.T) {
	fe := newFakeEngine(t)
	e := newOtelEngine(t, fe.m.handle)
	c := e.connect(t, WithName("observe-test"), otelOn(OtelConfig{}))
	waitUntil(t, "WorkerRegistered applied", c.registeredNow)
	registerLocal(t, c, "local::child", func(ctx context.Context, _ json.RawMessage) (any, error) { return 2, nil })

	parentCtx, parent := otel.Tracer("user").Start(context.Background(), "caller")
	if _, err := c.Trigger(parentCtx, TriggerRequest{FunctionID: "local::child"}); err != nil {
		t.Fatal(err)
	}
	parent.End()
	if invokeFrames(fe, "local::child") != 0 {
		t.Fatal("the call went through the engine")
	}
	child, _, _ := waitSpan(t, e.oc, "execute local::child")
	if child.TraceID != parent.SpanContext().TraceID().String() || child.ParentSpanID != parent.SpanContext().SpanID().String() {
		t.Errorf("local span trace %s parent %s, want %s / %s", child.TraceID, child.ParentSpanID, parent.SpanContext().TraceID(), parent.SpanContext().SpanID())
	}
}
