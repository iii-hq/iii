package telemetry

import (
	"context"

	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/baggage"
	"go.opentelemetry.io/otel/propagation"
	sdktrace "go.opentelemetry.io/otel/sdk/trace"
	"go.opentelemetry.io/otel/trace"
)

// Propagator is the W3C Trace Context + Baggage composite the Node SDK registers as its
// global propagator. The SDK uses it directly, so propagation works whether or not
// telemetry export is enabled or another global propagator is installed.
var Propagator propagation.TextMapPropagator = propagation.NewCompositeTextMapPropagator(
	propagation.TraceContext{},
	propagation.Baggage{},
)

// Inject returns the W3C traceparent and baggage headers for ctx; "" when ctx has no
// valid span context or no baggage.
func Inject(ctx context.Context) (traceparent, bag string) {
	carrier := propagation.MapCarrier{}
	Propagator.Inject(ctx, carrier)
	return carrier.Get("traceparent"), carrier.Get("baggage")
}

// Extract returns ctx carrying the remote span context of traceparent and the members
// of bag. Empty or invalid headers leave ctx unchanged.
func Extract(ctx context.Context, traceparent, bag string) context.Context {
	carrier := propagation.MapCarrier{}
	if traceparent != "" {
		carrier.Set("traceparent", traceparent)
	}
	if bag != "" {
		carrier.Set("baggage", bag)
	}
	if len(carrier) == 0 {
		return ctx
	}
	return Propagator.Extract(ctx, carrier)
}

// CurrentTraceID returns the trace id of the span in ctx as lowercase hex, "" when none.
func CurrentTraceID(ctx context.Context) string {
	if sc := trace.SpanContextFromContext(ctx); sc.TraceID().IsValid() {
		return sc.TraceID().String()
	}
	return ""
}

// CurrentSpanID returns the span id of the span in ctx as lowercase hex, "" when none.
func CurrentSpanID(ctx context.Context) string {
	if sc := trace.SpanContextFromContext(ctx); sc.SpanID().IsValid() {
		return sc.SpanID().String()
	}
	return ""
}

// GetBaggageEntry returns the value of the baggage member key in ctx.
func GetBaggageEntry(ctx context.Context, key string) (string, bool) {
	m := baggage.FromContext(ctx).Member(key)
	if m.Key() == "" {
		return "", false
	}
	return m.Value(), true
}

// SetBaggageEntry returns ctx with the baggage member key set to value.
func SetBaggageEntry(ctx context.Context, key, value string) (context.Context, error) {
	m, err := baggage.NewMemberRaw(key, value)
	if err != nil {
		return ctx, err
	}
	bag, err := baggage.FromContext(ctx).SetMember(m)
	if err != nil {
		return ctx, err
	}
	return baggage.ContextWithBaggage(ctx, bag), nil
}

// RemoveBaggageEntry returns ctx without the baggage member key.
func RemoveBaggageEntry(ctx context.Context, key string) context.Context {
	bag := baggage.FromContext(ctx)
	if bag.Len() == 0 {
		return ctx
	}
	return baggage.ContextWithBaggage(ctx, bag.DeleteMember(key))
}

// GetAllBaggage returns every baggage member in ctx as a key/value map.
func GetAllBaggage(ctx context.Context) map[string]string {
	members := baggage.FromContext(ctx).Members()
	out := make(map[string]string, len(members))
	for _, m := range members {
		out[m.Key()] = m.Value()
	}
	return out
}

// BaggageSpanProcessor copies every baggage member of a span's parent context onto the
// span as a string attribute when it starts (Node BaggageSpanProcessor). There is
// deliberately no key filtering: which attributes mean something is a query-side
// convention of the engine.
type BaggageSpanProcessor struct{}

// OnStart implements sdktrace.SpanProcessor.
func (BaggageSpanProcessor) OnStart(parent context.Context, s sdktrace.ReadWriteSpan) {
	if !s.IsRecording() {
		return
	}
	for _, m := range baggage.FromContext(parent).Members() {
		s.SetAttributes(attribute.String(m.Key(), m.Value()))
	}
}

// OnEnd implements sdktrace.SpanProcessor.
func (BaggageSpanProcessor) OnEnd(sdktrace.ReadOnlySpan) {}

// Shutdown implements sdktrace.SpanProcessor.
func (BaggageSpanProcessor) Shutdown(context.Context) error { return nil }

// ForceFlush implements sdktrace.SpanProcessor.
func (BaggageSpanProcessor) ForceFlush(context.Context) error { return nil }

var _ sdktrace.SpanProcessor = BaggageSpanProcessor{}
