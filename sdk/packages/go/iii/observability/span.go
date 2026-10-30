package observability

import (
	"context"

	otellog "go.opentelemetry.io/otel/log"
	"go.opentelemetry.io/otel/metric"
	"go.opentelemetry.io/otel/trace"

	"github.com/iii-hq/iii/sdk/packages/go/iii/internal/telemetry"
)

// Span is an OpenTelemetry span (Node exports the Span type the same way).
type Span = trace.Span

// Meter is an OpenTelemetry meter, as taken by [RegisterWorkerGauges].
type Meter = metric.Meter

// OtelLogger is an OpenTelemetry logger, as returned by [GetLogger].
type OtelLogger = otellog.Logger

// SpanOptions are the options of [WithSpan] (Node withSpan options).
type SpanOptions struct {
	// Kind is the span kind; the zero value means trace.SpanKindInternal.
	Kind trace.SpanKind
	// Traceparent, when set, is a W3C traceparent header used as the parent instead of
	// the span in ctx.
	Traceparent string
}

// WithSpan runs fn inside a new span named name and returns fn's result (Node
// withSpan). The span is a child of opts.Traceparent when set, else of the span in ctx;
// fn receives the span and a ctx carrying it, to pass on to Trigger, the Logger or
// nested spans. The span ends with an OK status when fn returns a nil error, and with an
// ERROR status and an exception event when fn returns an error or panics (the panic is
// re-raised). Without running telemetry the global tracer provider is used.
//
//	total, err := observability.WithSpan(ctx, "compute-total", observability.SpanOptions{},
//	    func(ctx context.Context, span observability.Span) (float64, error) {
//	        return sum(order.Items), nil
//	    })
func WithSpan[T any](ctx context.Context, name string, opts SpanOptions, fn func(ctx context.Context, span Span) (T, error)) (T, error) {
	return telemetry.WithSpan(ctx, name, telemetry.SpanOptions{Kind: opts.Kind, Traceparent: opts.Traceparent}, fn)
}

// CurrentSpanIsRecording reports whether ctx holds a span that records; false when there
// is none or the sampler dropped it (Node currentSpanIsRecording).
func CurrentSpanIsRecording(ctx context.Context) bool { return telemetry.CurrentSpanIsRecording(ctx) }

// SetCurrentSpanAttribute sets an attribute on the span in ctx; a no-op when it does not
// record (Node setCurrentSpanAttribute). Strings, booleans, integers, floats and slices
// of them keep their type; other values are recorded as text.
func SetCurrentSpanAttribute(ctx context.Context, key string, value any) {
	telemetry.SetCurrentSpanAttribute(ctx, key, value)
}

// SetCurrentSpanError sets an ERROR status with message on the span in ctx; a no-op when
// there is none (Node setCurrentSpanError).
func SetCurrentSpanError(ctx context.Context, message string) {
	telemetry.SetCurrentSpanError(ctx, message)
}

// RecordSpanEvent adds an event with optional attributes to the span in ctx; a no-op
// when it does not record (Node recordSpanEvent). Attribute values convert as in
// [SetCurrentSpanAttribute].
func RecordSpanEvent(ctx context.Context, name string, attrs map[string]any) {
	telemetry.RecordSpanEvent(ctx, name, attrs)
}

// GetLogger returns the OpenTelemetry logger of the running client telemetry, or nil
// when there is none (Node getLogger). Most code should use [Logger] instead.
func GetLogger() OtelLogger {
	if l := telemetry.LogLogger(); l != nil {
		return l
	}
	return nil
}

// Flush exports the spans, metrics and log records buffered by the running client
// telemetry without stopping it (Node flushOtel); nil when none is running. Use it
// before a short-lived process exits without closing its client.
func Flush(ctx context.Context) error {
	if s := telemetry.Active(); s != nil {
		return s.Flush(ctx)
	}
	return nil
}
