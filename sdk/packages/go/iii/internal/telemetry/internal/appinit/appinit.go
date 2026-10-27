// Package appinit is a test fixture for the SDK's rule on the OpenTelemetry globals
// (review round 2, finding B): an application package that installs its own tracer
// provider and propagator in an init() which the Go runtime runs before
// internal/telemetry initializes.
//
// Since Go 1.21, packages initialize in import-path order among those whose imports are
// already initialized. This package imports only go.opentelemetry.io/otel,
// otel/propagation, otel/trace and otel/trace/embedded, all of which internal/telemetry
// needs too (through otel/sdk/trace, among others), while internal/telemetry also waits
// for otel/sdk/*. So appinit becomes ready first, and it sorts before every
// go.opentelemetry.io package telemetry still waits for: its init() runs before
// telemetry's package variables are initialized, as a program's tracing bootstrap in an
// init() would. TestApplicationGlobalsSetInInitAreKept (appinit_test.go) relies on it.
// No package of the SDK imports it.
package appinit

import (
	"context"

	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/propagation"
	"go.opentelemetry.io/otel/trace"
	"go.opentelemetry.io/otel/trace/embedded"
)

// AppTracerProvider is the application's tracer provider, installed by init().
type AppTracerProvider struct{ embedded.TracerProvider }

// Tracer returns a tracer that starts no span of its own.
func (*AppTracerProvider) Tracer(string, ...trace.TracerOption) trace.Tracer { return appTracer{} }

type appTracer struct{ embedded.Tracer }

func (appTracer) Start(ctx context.Context, _ string, _ ...trace.SpanStartOption) (context.Context, trace.Span) {
	return ctx, trace.SpanFromContext(ctx)
}

// TracerProvider is the application's global tracer provider.
var TracerProvider = &AppTracerProvider{}

// Propagator is the application's global propagator: a value type, which the round 1
// value comparison mistook for the otel default.
var Propagator = propagation.TraceContext{}

func init() {
	otel.SetTracerProvider(TracerProvider)
	otel.SetTextMapPropagator(Propagator)
}
