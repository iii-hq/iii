package telemetry

import (
	"context"
	"fmt"
	"math"

	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/codes"
	"go.opentelemetry.io/otel/trace"
)

// Tracer returns the tracer of the active System (its instrumentation scope is the
// service name, as Node trace.getTracer(serviceName)), or a tracer of the global
// provider when no client has telemetry running.
func Tracer() trace.Tracer {
	if s := Active(); s != nil {
		return s.tracer
	}
	return otel.GetTracerProvider().Tracer(DefaultServiceName)
}

// CurrentSpanIsRecording reports whether ctx holds a recording span (Node
// currentSpanIsRecording).
func CurrentSpanIsRecording(ctx context.Context) bool {
	return trace.SpanFromContext(ctx).IsRecording()
}

// SetCurrentSpanAttribute sets key on the span in ctx; a no-op when it is not recording.
func SetCurrentSpanAttribute(ctx context.Context, key string, value any) {
	span := trace.SpanFromContext(ctx)
	if !span.IsRecording() {
		return
	}
	span.SetAttributes(ToAttribute(key, value))
}

// SetCurrentSpanError marks the span in ctx as failed with message.
func SetCurrentSpanError(ctx context.Context, message string) {
	trace.SpanFromContext(ctx).SetStatus(codes.Error, message)
}

// RecordSpanEvent adds an event to the span in ctx; a no-op when it is not recording.
func RecordSpanEvent(ctx context.Context, name string, attrs map[string]any) {
	span := trace.SpanFromContext(ctx)
	if !span.IsRecording() {
		return
	}
	span.AddEvent(name, trace.WithAttributes(ToAttributes(attrs)...))
}

// ToAttributes converts a map with ToAttribute; key order is not significant.
func ToAttributes(attrs map[string]any) []attribute.KeyValue {
	out := make([]attribute.KeyValue, 0, len(attrs))
	for k, v := range attrs {
		out = append(out, ToAttribute(k, v))
	}
	return out
}

// ToAttribute converts a Go value to a span attribute: strings, booleans, integers,
// floats and slices of them keep their type, an attribute.Value is used as is, a
// fmt.Stringer becomes its String() and anything else its fmt.Sprint text.
func ToAttribute(key string, v any) attribute.KeyValue {
	k := attribute.Key(key)
	switch x := v.(type) {
	case attribute.Value:
		return attribute.KeyValue{Key: k, Value: x}
	case string:
		return k.String(x)
	case bool:
		return k.Bool(x)
	case int:
		return k.Int(x)
	case int8:
		return k.Int64(int64(x))
	case int16:
		return k.Int64(int64(x))
	case int32:
		return k.Int64(int64(x))
	case int64:
		return k.Int64(x)
	case uint8:
		return k.Int64(int64(x))
	case uint16:
		return k.Int64(int64(x))
	case uint32:
		return k.Int64(int64(x))
	case uint:
		if uint64(x) <= math.MaxInt64 {
			return k.Int64(int64(x))
		}
		return k.String(fmt.Sprint(x))
	case uint64:
		if x <= math.MaxInt64 {
			return k.Int64(int64(x))
		}
		return k.String(fmt.Sprint(x))
	case float32:
		return k.Float64(float64(x))
	case float64:
		return k.Float64(x)
	case []string:
		return k.StringSlice(x)
	case []bool:
		return k.BoolSlice(x)
	case []int:
		return k.IntSlice(x)
	case []int64:
		return k.Int64Slice(x)
	case []float64:
		return k.Float64Slice(x)
	case fmt.Stringer:
		return k.String(x.String())
	case nil:
		return k.String("")
	default:
		return k.String(fmt.Sprint(x))
	}
}

// SpanOptions are the options of WithSpan (Node withSpan options).
type SpanOptions struct {
	Kind        trace.SpanKind
	Traceparent string
}

// WithSpan runs fn inside a new span (Node withSpan): kind defaults to internal, the
// parent is opts.Traceparent when set and ctx otherwise. The span ends OK when fn
// returns a nil error, and with an ERROR status and an exception event when fn returns
// an error or panics (the panic is re-raised).
func WithSpan[T any](ctx context.Context, name string, opts SpanOptions, fn func(ctx context.Context, span trace.Span) (T, error)) (result T, err error) {
	kind := opts.Kind
	if kind == trace.SpanKindUnspecified {
		kind = trace.SpanKindInternal
	}
	parent := ctx
	if opts.Traceparent != "" {
		parent = Extract(ctx, opts.Traceparent, "")
	}
	ctx, span := Tracer().Start(parent, name, trace.WithSpanKind(kind))
	defer func() {
		if r := recover(); r != nil {
			perr := fmt.Errorf("panic: %v", r)
			span.SetStatus(codes.Error, perr.Error())
			span.RecordError(perr)
			span.End()
			panic(r)
		}
		if err != nil {
			span.SetStatus(codes.Error, err.Error())
			span.RecordError(err)
		} else {
			span.SetStatus(codes.Ok, "")
		}
		span.End()
	}()
	return fn(ctx, span)
}
