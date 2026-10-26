package telemetry

import (
	"context"
	"reflect"
	"slices"
	"testing"

	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/metric"
	"go.opentelemetry.io/otel/propagation"
	sdkmetric "go.opentelemetry.io/otel/sdk/metric"
	sdktrace "go.opentelemetry.io/otel/sdk/trace"
	"go.opentelemetry.io/otel/trace"
)

// sorted returns a sorted copy: a composite propagator does not promise a field order.
func sorted(fields []string) []string {
	out := slices.Clone(fields)
	slices.Sort(out)
	return out
}

func requireDefaultGlobals(t *testing.T, when string) {
	t.Helper()
	if otel.GetTracerProvider() != defaultTracerProvider || otel.GetMeterProvider() != defaultMeterProvider || otel.GetTextMapPropagator() != defaultPropagator {
		t.Fatalf("%s: OpenTelemetry globals are not the otel defaults", when)
	}
}

// First registration wins: the first System installs the globals, a second one leaves
// them alone, and each Shutdown restores only what its System installed.
func TestGlobalsFirstRegistrationWinsAndRestore(t *testing.T) {
	requireDefaultGlobals(t, "before Start")
	f := newFakeOtelEngine(t, false)
	ctx := context.Background()

	a := startTestSystem(t, f, Options{ServiceName: "a"})
	if otel.GetTracerProvider() != trace.TracerProvider(a.tp) || otel.GetMeterProvider() != metric.MeterProvider(a.mp) {
		t.Fatal("the first System did not install its providers")
	}
	if p, ok := otel.GetTextMapPropagator().(*globalPropagator); !ok || !reflect.DeepEqual(sorted(p.Fields()), sorted(Propagator.Fields())) {
		t.Fatalf("global propagator %T", otel.GetTextMapPropagator())
	}

	b := startTestSystem(t, f, Options{ServiceName: "b"})
	if otel.GetTracerProvider() != trace.TracerProvider(a.tp) || otel.GetMeterProvider() != metric.MeterProvider(a.mp) || otel.GetTextMapPropagator() != propagation.TextMapPropagator(a.globals.propagator) {
		t.Fatal("the second System replaced the first one's globals")
	}
	if Active() != b {
		t.Fatal("the System started last is not active")
	}

	_ = b.Shutdown(ctx)
	if otel.GetTracerProvider() != trace.TracerProvider(a.tp) || otel.GetMeterProvider() != metric.MeterProvider(a.mp) {
		t.Fatal("the second System's Shutdown changed globals it did not install")
	}
	if Active() != a {
		t.Fatal("the remaining System did not become active again")
	}

	_ = a.Shutdown(ctx)
	requireDefaultGlobals(t, "after the last Shutdown")
	if Active() != nil {
		t.Fatal("a System is still active")
	}
}

// An application's provider and propagator, installed before Start, are never
// replaced, not even by Shutdown. The composite propagator is a slice type: the
// comparisons must not panic on it.
func TestGlobalsNeverReplaceTheApplications(t *testing.T) {
	requireDefaultGlobals(t, "before the test")
	appTP := sdktrace.NewTracerProvider()
	appMP := sdkmetric.NewMeterProvider()
	appProp := propagation.NewCompositeTextMapPropagator(propagation.Baggage{})
	otel.SetTracerProvider(appTP)
	otel.SetMeterProvider(appMP)
	otel.SetTextMapPropagator(appProp)
	t.Cleanup(func() {
		otel.SetTracerProvider(defaultTracerProvider)
		otel.SetMeterProvider(defaultMeterProvider)
		otel.SetTextMapPropagator(defaultPropagator)
	})
	check := func(when string) {
		t.Helper()
		if otel.GetTracerProvider() != trace.TracerProvider(appTP) || otel.GetMeterProvider() != metric.MeterProvider(appMP) {
			t.Errorf("%s: application providers replaced", when)
		}
		if got := otel.GetTextMapPropagator().Fields(); !reflect.DeepEqual(got, []string{"baggage"}) {
			t.Errorf("%s: application propagator replaced: fields %v", when, got)
		}
	}

	s := startTestSystem(t, newFakeOtelEngine(t, false), Options{})
	check("after Start")
	if s.globals != (installedGlobals{}) {
		t.Errorf("System recorded installed globals %+v", s.globals)
	}
	_ = s.Shutdown(context.Background())
	check("after Shutdown")
}

// A global the application replaces after Start is left alone by Shutdown.
func TestGlobalsReplacedAfterStartAreKept(t *testing.T) {
	requireDefaultGlobals(t, "before the test")
	s := startTestSystem(t, newFakeOtelEngine(t, false), Options{})
	appTP := sdktrace.NewTracerProvider()
	otel.SetTracerProvider(appTP)
	t.Cleanup(func() { otel.SetTracerProvider(defaultTracerProvider) })
	_ = s.Shutdown(context.Background())
	if otel.GetTracerProvider() != trace.TracerProvider(appTP) {
		t.Error("Shutdown replaced the application's tracer provider")
	}
	if otel.GetMeterProvider() != defaultMeterProvider || otel.GetTextMapPropagator() != defaultPropagator {
		t.Error("Shutdown did not restore the globals it still held")
	}
}
