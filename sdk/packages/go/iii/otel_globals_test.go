package iii

import (
	"context"
	"encoding/json"
	"reflect"
	"sync"
	"testing"
	"time"

	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/metric"
	"go.opentelemetry.io/otel/propagation"
	sdkmetric "go.opentelemetry.io/otel/sdk/metric"
	sdktrace "go.opentelemetry.io/otel/sdk/trace"
	"go.opentelemetry.io/otel/sdk/trace/tracetest"
	"go.opentelemetry.io/otel/trace"

	"github.com/iii-hq/iii/sdk/packages/go/iii/internal/telemetry"
)

// Review round 1, finding 2: an application that set its own tracer provider, meter
// provider and propagator before RegisterWorker keeps them through the client's whole
// life and after Close, and the SDK's own invocation spans still reach the engine
// through the client's provider.
func TestOtelKeepsApplicationGlobals(t *testing.T) {
	prevTP, prevMP, prevProp := otel.GetTracerProvider(), otel.GetMeterProvider(), otel.GetTextMapPropagator()
	rec := tracetest.NewSpanRecorder()
	appTP := sdktrace.NewTracerProvider(sdktrace.WithSpanProcessor(rec))
	appMP := sdkmetric.NewMeterProvider()
	// A composite propagator is a slice type: the SDK's comparisons must not panic on it.
	appProp := propagation.NewCompositeTextMapPropagator(propagation.Baggage{})
	otel.SetTracerProvider(appTP)
	otel.SetMeterProvider(appMP)
	otel.SetTextMapPropagator(appProp)
	t.Cleanup(func() {
		otel.SetTracerProvider(prevTP)
		otel.SetMeterProvider(prevMP)
		otel.SetTextMapPropagator(prevProp)
	})
	checkGlobals := func(when string) {
		t.Helper()
		if otel.GetTracerProvider() != trace.TracerProvider(appTP) {
			t.Errorf("%s: the application's tracer provider was replaced by %T", when, otel.GetTracerProvider())
		}
		if otel.GetMeterProvider() != metric.MeterProvider(appMP) {
			t.Errorf("%s: the application's meter provider was replaced by %T", when, otel.GetMeterProvider())
		}
		if got := otel.GetTextMapPropagator().Fields(); !reflect.DeepEqual(got, []string{"baggage"}) {
			t.Errorf("%s: the application's propagator was replaced: fields %v", when, got)
		}
	}

	e := newOtelEngine(t, nil)
	e.invokeOnRegister(t, "app::fn", `{}`, testTraceparent, "")
	c := e.connect(t, otelOn(OtelConfig{}))
	_ = c.RegisterFunction("app::fn", func(ctx context.Context, _ json.RawMessage) (any, error) {
		_, span := otel.Tracer("app").Start(ctx, "app-work")
		span.End()
		return 1, nil
	})

	execute, _, _ := waitSpan(t, e.oc, "execute app::fn")
	if execute.TraceID != testTraceID || execute.ParentSpanID != testSpanID {
		t.Errorf("execute span trace %s parent %s", execute.TraceID, execute.ParentSpanID)
	}
	checkGlobals("after RegisterWorker")

	// The application's span went to the application's provider, parented to the
	// invocation span through ctx, and not to the engine.
	ended := rec.Ended()
	if len(ended) != 1 || ended[0].Name() != "app-work" || ended[0].Parent().SpanID().String() != execute.SpanID ||
		ended[0].SpanContext().TraceID().String() != testTraceID {
		t.Errorf("application spans %v", ended)
	}
	if _, _, _, ok := findSpan(e.oc, "app-work"); ok {
		t.Error("the application's span was exported to the engine")
	}

	_ = c.Close()
	checkGlobals("after Close")
}

// Finding 2: a client installs the globals only while they are the otel defaults, the
// first client wins, and Close puts back the defaults it replaced.
func TestOtelRestoresGlobalsOnClose(t *testing.T) {
	defTP, defMP, defProp := otel.GetTracerProvider(), otel.GetMeterProvider(), otel.GetTextMapPropagator()
	e := newOtelEngine(t, nil)

	first := e.connect(t, otelOn(OtelConfig{ServiceName: "first"}))
	firstTP, firstMP, firstProp := otel.GetTracerProvider(), otel.GetMeterProvider(), otel.GetTextMapPropagator()
	if firstTP == defTP || firstMP == defMP || firstProp == defProp {
		t.Fatal("the first client did not install its provider and propagator over the otel defaults")
	}

	second := e.connect(t, otelOn(OtelConfig{ServiceName: "second"}))
	if otel.GetTracerProvider() != firstTP || otel.GetMeterProvider() != firstMP || otel.GetTextMapPropagator() != firstProp {
		t.Fatal("the second client replaced the first client's globals")
	}
	_ = second.Close()
	if otel.GetTracerProvider() != firstTP || otel.GetMeterProvider() != firstMP || otel.GetTextMapPropagator() != firstProp {
		t.Fatal("closing the second client changed the first client's globals")
	}

	_ = first.Close()
	if otel.GetTracerProvider() != defTP || otel.GetMeterProvider() != defMP || otel.GetTextMapPropagator() != defProp {
		t.Fatal("Close did not restore the otel defaults")
	}
	if telemetry.Active() != nil {
		t.Error("a telemetry pipeline is still active")
	}
}

// Review round 1, finding 9: once Close has stopped telemetry, a startTelemetry racing
// it (the first Connect running before Close closes shutdown) starts nothing.
func TestOtelStartAfterStopIsNoop(t *testing.T) {
	e := newOtelEngine(t, nil)

	// The window, made deterministic: Close's stopTelemetry has run, then the first
	// Connect's startTelemetry runs before Close closes shutdown.
	c := New(e.url, otelOn(OtelConfig{}))
	c.stopTelemetry()
	c.startTelemetry()
	if c.telemetrySystem() != nil {
		t.Fatal("startTelemetry after stopTelemetry started a pipeline")
	}
	_ = c.Close()
	time.Sleep(50 * time.Millisecond)
	if n := e.oc.connections(); n != 0 {
		t.Fatalf("%d /otel connections from a client closed before its first Connect", n)
	}

	// The race itself, repeated.
	for i := 0; i < 50; i++ {
		c := New(e.url, otelOn(OtelConfig{}))
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		var wg sync.WaitGroup
		wg.Add(2)
		go func() { defer wg.Done(); _ = c.Close() }()
		go func() { defer wg.Done(); _ = c.Connect(ctx) }()
		wg.Wait()
		cancel()
		if c.telemetrySystem() != nil {
			t.Fatalf("iteration %d: telemetry left running after Close", i)
		}
	}
	if telemetry.Active() != nil {
		t.Error("a telemetry pipeline is still active after every client closed")
	}
}
