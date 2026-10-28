package telemetry

import (
	"bytes"
	"context"
	"encoding/json"
	"testing"
	"time"

	"go.opentelemetry.io/otel"
)

func startTestSystem(t *testing.T, f *fakeOtelEngine, o Options) *System {
	t.Helper()
	o.EngineWSURL = f.url
	if o.Enabled == nil {
		o.Enabled = boolPtr(true)
	}
	if o.ServiceName == "" {
		o.ServiceName = "sys-test"
	}
	cfg, err := Resolve(o, envMap(nil))
	if err != nil {
		t.Fatal(err)
	}
	s, err := Start(cfg)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = s.Shutdown(context.Background()) })
	return s
}

func framesWithPrefix(frames [][]byte, prefix string) [][]byte {
	var out [][]byte
	for _, f := range frames {
		if bytes.HasPrefix(f, []byte(prefix)) {
			out = append(out, f[len(prefix):])
		}
	}
	return out
}

// Start registers the globals, Shutdown flushes all three signals over the socket and
// closes it, and the System stops being the active one.
func TestSystemShutdownFlushes(t *testing.T) {
	f := newFakeOtelEngine(t, false)
	hour := time.Hour
	s := startTestSystem(t, f, Options{SpansFlushInterval: hour, LogsFlushInterval: hour, MetricsExportInterval: hour, LogsBatchSize: 512})
	if Active() != s || otel.GetTracerProvider() != s.tp {
		t.Fatal("Start did not register the System")
	}
	waitState(t, s.conn, StateConnected)

	_, span := otel.Tracer("user").Start(context.Background(), "user-span")
	span.End()
	counter, _ := otel.Meter("user").Int64Counter("user.counter")
	counter.Add(context.Background(), 2)
	EmitLog(context.Background(), SeverityWarn, "flushed on shutdown", "", "", "", nil)

	time.Sleep(50 * time.Millisecond)
	if n := len(f.snapshot()); n != 0 {
		t.Fatalf("%d frames before shutdown with 1h flush intervals", n)
	}
	if err := s.Shutdown(context.Background()); err != nil {
		t.Fatal(err)
	}
	frames := f.snapshot()
	spans, metrics, logs := framesWithPrefix(frames, PrefixTraces), framesWithPrefix(frames, PrefixMetrics), framesWithPrefix(frames, PrefixLogs)
	if len(spans) != 1 || !bytes.Contains(spans[0], []byte(`"name":"user-span"`)) {
		t.Errorf("span frames %q", spans)
	}
	if len(metrics) == 0 || !bytes.Contains(metrics[0], []byte(`"name":"user.counter"`)) || !bytes.Contains(metrics[0], []byte(`"asInt":2`)) {
		t.Errorf("metric frames %q", metrics)
	}
	if len(logs) != 1 || !bytes.Contains(logs[0], []byte(`"severityNumber":13`)) || !bytes.Contains(logs[0], []byte(`"stringValue":"flushed on shutdown"`)) {
		t.Errorf("log frames %q", logs)
	}
	f.waitFor(t, "socket close", func() bool { return f.closed == 1 })
	if Active() != nil {
		t.Error("still active after Shutdown")
	}
	requireDefaultGlobals(t, "after Shutdown")
	if err := s.Shutdown(context.Background()); err != nil {
		t.Errorf("second Shutdown: %v", err)
	}
}

// The resource carries the service attributes; every signal uses the service name as its
// instrumentation scope (Node getTracer/getMeter/getLogger(serviceName)).
func TestSystemResourceAndScope(t *testing.T) {
	t.Setenv("OTEL_RESOURCE_ATTRIBUTES", "")
	f := newFakeOtelEngine(t, false)
	s := startTestSystem(t, f, Options{ServiceVersion: "9.9.9", ServiceNamespace: "ns", ServiceInstanceID: "i-1"})
	_, span := s.Tracer().Start(context.Background(), "scoped")
	span.End()
	if err := s.Flush(context.Background()); err != nil {
		t.Fatal(err)
	}
	f.waitFor(t, "span frame", func() bool { return len(framesWithPrefix(f.frames, PrefixTraces)) == 1 })
	var req struct {
		ResourceSpans []struct {
			Resource struct {
				Attributes []struct {
					Key   string
					Value struct{ StringValue string }
				}
			}
			ScopeSpans []struct {
				Scope struct{ Name string }
			}
		}
	}
	if err := json.Unmarshal(framesWithPrefix(f.snapshot(), PrefixTraces)[0], &req); err != nil {
		t.Fatal(err)
	}
	attrs := map[string]string{}
	for _, a := range req.ResourceSpans[0].Resource.Attributes {
		attrs[a.Key] = a.Value.StringValue
	}
	want := map[string]string{"service.name": "sys-test", "service.version": "9.9.9", "service.namespace": "ns", "service.instance.id": "i-1"}
	for k, v := range want {
		if attrs[k] != v {
			t.Errorf("resource %s = %q, want %q (all: %v)", k, attrs[k], v, attrs)
		}
	}
	if got := req.ResourceSpans[0].ScopeSpans[0].Scope.Name; got != "sys-test" {
		t.Errorf("scope %q", got)
	}
}

func TestStartRefusesDisabledConfig(t *testing.T) {
	if s, err := Start(Config{}); err == nil || s != nil {
		t.Errorf("Start(disabled) = %v, %v", s, err)
	}
}

// Without an active System the Logger falls back to the standard logger.
func TestEmitLogWithoutSystem(t *testing.T) {
	if Active() != nil {
		t.Skip("a System is active")
	}
	EmitLog(context.Background(), SeverityInfo, "to stderr", "", "", "", map[string]int{"a": 1})
	if LogLogger() != nil {
		t.Error("LogLogger without a System")
	}
}
