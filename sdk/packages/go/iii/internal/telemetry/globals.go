package telemetry

import (
	"sync"
	"sync/atomic"

	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/metric"
	"go.opentelemetry.io/otel/propagation"
	"go.opentelemetry.io/otel/trace"
)

// The OpenTelemetry globals as the otel package starts them: delegating defaults that
// do nothing until a program installs a real provider or propagator. Captured when this
// package initializes, before any program code runs (a package initializes after its
// dependencies and before its importers).
var (
	defaultTracerProvider = otel.GetTracerProvider()
	defaultMeterProvider  = otel.GetMeterProvider()
	defaultPropagator     = otel.GetTextMapPropagator()
)

// globalsMu orders every check-and-set of the OpenTelemetry globals by this package.
var globalsMu sync.Mutex

// globalPropagator wraps the propagator installed as the global one. The pointer is what
// identifies it on Shutdown: the composite propagator is a slice type, and comparing two
// interface values holding the same non-comparable type panics.
type globalPropagator struct {
	propagation.TextMapPropagator
}

// installedGlobals records which globals a System installed, to restore exactly those.
type installedGlobals struct {
	tracerProvider bool
	meterProvider  bool
	propagator     *globalPropagator
}

// installGlobals makes the System's tracer provider, meter provider (when metrics are
// enabled) and the W3C TraceContext + Baggage propagator the OpenTelemetry globals, each
// only while that global is still the otel default. A provider or propagator the
// application (or another client) installed first is never replaced: first registration
// wins, as with the Node SDK's @opentelemetry/api setGlobalTracerProvider,
// setGlobalMeterProvider and setGlobalPropagator. The SDK's own spans, metrics and logs
// use the System's providers directly, so they reach the engine either way.
func (s *System) installGlobals() {
	globalsMu.Lock()
	defer globalsMu.Unlock()
	// Each comparison has a pointer type on one side, so it cannot panic.
	if otel.GetTracerProvider() == defaultTracerProvider {
		otel.SetTracerProvider(s.tp)
		s.globals.tracerProvider = true
	}
	if s.mp != nil && otel.GetMeterProvider() == defaultMeterProvider {
		otel.SetMeterProvider(s.mp)
		s.globals.meterProvider = true
	}
	if otel.GetTextMapPropagator() == defaultPropagator {
		p := &globalPropagator{Propagator}
		otel.SetTextMapPropagator(p)
		s.globals.propagator = p
	}
}

// releaseGlobals puts the otel default back into every global that still holds what this
// System installed; a global the application replaced since is left alone.
//
// The otel package delegates its default provider to the first provider ever installed,
// once and for good: a tracer or meter taken from the default before that keeps
// delegating to this System's provider, which records nothing once shut down.
func (s *System) releaseGlobals() {
	globalsMu.Lock()
	defer globalsMu.Unlock()
	if s.globals.tracerProvider && otel.GetTracerProvider() == trace.TracerProvider(s.tp) {
		otel.SetTracerProvider(defaultTracerProvider)
	}
	if s.globals.meterProvider && otel.GetMeterProvider() == metric.MeterProvider(s.mp) {
		otel.SetMeterProvider(defaultMeterProvider)
	}
	if p := s.globals.propagator; p != nil && otel.GetTextMapPropagator() == propagation.TextMapPropagator(p) {
		otel.SetTextMapPropagator(defaultPropagator)
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
