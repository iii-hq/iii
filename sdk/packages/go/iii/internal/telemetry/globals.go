package telemetry

import (
	"reflect"
	"sync"
	"sync/atomic"

	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/metric"
	"go.opentelemetry.io/otel/propagation"
	"go.opentelemetry.io/otel/trace"
)

// otelGlobalPkg holds the otel package's delegating defaults: until a program installs
// a provider or propagator, otel.GetTracerProvider, otel.GetMeterProvider and
// otel.GetTextMapPropagator return a *tracerProvider, *meterProvider and
// *textMapPropagator of this package (go.opentelemetry.io/otel v1.46.0
// internal/global/{trace,meter,propagator}.go). TestGlobalsOtelDefaultPredicates fails if an
// otel upgrade renames them.
const otelGlobalPkg = "go.opentelemetry.io/otel/internal/global"

// isOtelDefault reports whether v is the otel default of the given type name: a pointer
// to that unexported type of otelGlobalPkg. The decision rests on the dynamic type, not
// on a value captured at some point: an application can install its globals in any
// init(), including one the Go runtime runs before this package initializes.
func isOtelDefault(v any, typeName string) bool {
	t := reflect.TypeOf(v)
	if t == nil || t.Kind() != reflect.Pointer {
		return false
	}
	t = t.Elem()
	return t.PkgPath() == otelGlobalPkg && t.Name() == typeName
}

// isDefaultTracerProvider reports whether tp is the otel default tracer provider.
func isDefaultTracerProvider(tp trace.TracerProvider) bool {
	return isOtelDefault(tp, "tracerProvider")
}

// isDefaultMeterProvider reports whether mp is the otel default meter provider.
func isDefaultMeterProvider(mp metric.MeterProvider) bool {
	return isOtelDefault(mp, "meterProvider")
}

// isDefaultPropagator reports whether p is the otel default propagator.
func isDefaultPropagator(p propagation.TextMapPropagator) bool {
	return isOtelDefault(p, "textMapPropagator")
}

// globalsMu orders every check-and-set of the OpenTelemetry globals by this package.
var globalsMu sync.Mutex

// globalPropagator wraps the propagator installed as the global one. The pointer is what
// identifies it on Shutdown: the composite propagator is a slice type, and comparing two
// interface values holding the same non-comparable type panics.
type globalPropagator struct {
	propagation.TextMapPropagator
}

// installedGlobals records, for each global a System installed, the value that was
// global just before, to restore exactly that value. A nil field means "not installed".
type installedGlobals struct {
	prevTracerProvider trace.TracerProvider
	prevMeterProvider  metric.MeterProvider
	prevPropagator     propagation.TextMapPropagator
	propagator         *globalPropagator
}

// installGlobals makes the System's tracer provider, meter provider (when metrics are
// enabled) and the W3C TraceContext + Baggage propagator the OpenTelemetry globals, each
// only while that global is still the otel default. A provider or propagator the
// application (or another client) installed first is never replaced, whenever it did
// so: first registration wins, as with the Node SDK's @opentelemetry/api
// setGlobalTracerProvider, setGlobalMeterProvider and setGlobalPropagator. The SDK's own
// spans, metrics and logs use the System's providers directly, so they reach the engine
// either way.
func (s *System) installGlobals() {
	globalsMu.Lock()
	defer globalsMu.Unlock()
	if tp := otel.GetTracerProvider(); isDefaultTracerProvider(tp) {
		s.globals.prevTracerProvider = tp
		otel.SetTracerProvider(s.tp)
	}
	if mp := otel.GetMeterProvider(); s.mp != nil && isDefaultMeterProvider(mp) {
		s.globals.prevMeterProvider = mp
		otel.SetMeterProvider(s.mp)
	}
	if prop := otel.GetTextMapPropagator(); isDefaultPropagator(prop) {
		p := &globalPropagator{Propagator}
		s.globals.prevPropagator = prop
		s.globals.propagator = p
		otel.SetTextMapPropagator(p)
	}
}

// releaseGlobals puts back the value that was global before installGlobals into every
// global that still holds what this System installed; a global the application replaced
// meanwhile is left alone. Each comparison has a pointer type on one side, so it cannot
// panic.
//
// The otel package delegates its default provider to the first provider ever installed,
// once and for good: a tracer or meter taken from the default before that keeps
// delegating to this System's provider, which records nothing once shut down.
func (s *System) releaseGlobals() {
	globalsMu.Lock()
	defer globalsMu.Unlock()
	if prev := s.globals.prevTracerProvider; prev != nil && otel.GetTracerProvider() == trace.TracerProvider(s.tp) {
		otel.SetTracerProvider(prev)
	}
	if prev := s.globals.prevMeterProvider; prev != nil && otel.GetMeterProvider() == metric.MeterProvider(s.mp) {
		otel.SetMeterProvider(prev)
	}
	if p := s.globals.propagator; p != nil && otel.GetTextMapPropagator() == propagation.TextMapPropagator(p) {
		otel.SetTextMapPropagator(s.globals.prevPropagator)
	}
	s.globals = installedGlobals{}
}

// The running Systems, in start order. The last one is the active System: what the
// package-level helpers (Logger, WithSpan, the HTTP transport, Flush) use, as the Node
// helpers use their module state. When it shuts down, the one started before it (if
// still running) becomes active again.
var (
	runningMu sync.Mutex
	running   []*System
	active    atomic.Pointer[System]
)

// Active returns the running System started last, or nil.
func Active() *System { return active.Load() }

func register(s *System) {
	runningMu.Lock()
	defer runningMu.Unlock()
	running = append(running, s)
	active.Store(s)
}

func unregister(s *System) {
	runningMu.Lock()
	defer runningMu.Unlock()
	for i, r := range running {
		if r == s {
			running = append(running[:i:i], running[i+1:]...)
			break
		}
	}
	if len(running) == 0 {
		active.Store(nil)
		return
	}
	active.Store(running[len(running)-1])
}
