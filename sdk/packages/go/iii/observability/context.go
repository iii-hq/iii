package observability

import (
	"context"

	"github.com/iii-hq/iii/sdk/packages/go/iii/internal/telemetry"
)

// CurrentTraceID returns the trace id of the span in ctx as 32 lowercase hex digits, or
// "" when ctx has none (Node currentTraceId).
func CurrentTraceID(ctx context.Context) string { return telemetry.CurrentTraceID(ctx) }

// CurrentSpanID returns the span id of the span in ctx as 16 lowercase hex digits, or
// "" when ctx has none (Node currentSpanId).
func CurrentSpanID(ctx context.Context) string { return telemetry.CurrentSpanID(ctx) }

// InjectTraceparent returns the W3C traceparent header for the span in ctx, or "" when
// ctx has none (Node injectTraceparent).
func InjectTraceparent(ctx context.Context) string {
	tp, _ := telemetry.Inject(ctx)
	return tp
}

// InjectBaggage returns the W3C baggage header for the baggage in ctx, or "" when ctx
// has none (Node injectBaggage).
func InjectBaggage(ctx context.Context) string {
	_, bag := telemetry.Inject(ctx)
	return bag
}

// ExtractTraceparent returns ctx with the remote span context of a W3C traceparent
// header as its parent; an invalid header leaves ctx unchanged (Node
// extractTraceparent).
func ExtractTraceparent(ctx context.Context, traceparent string) context.Context {
	return telemetry.Extract(ctx, traceparent, "")
}

// ExtractBaggage returns ctx with the members of a W3C baggage header; an invalid header
// leaves ctx unchanged (Node extractBaggage).
func ExtractBaggage(ctx context.Context, baggage string) context.Context {
	return telemetry.Extract(ctx, "", baggage)
}

// ExtractContext returns ctx with both a traceparent and a baggage header applied;
// either may be empty (Node extractContext).
func ExtractContext(ctx context.Context, traceparent, baggage string) context.Context {
	return telemetry.Extract(ctx, traceparent, baggage)
}

// GetBaggageEntry returns the value of the baggage member key in ctx and whether it is
// present (Node getBaggageEntry).
func GetBaggageEntry(ctx context.Context, key string) (string, bool) {
	return telemetry.GetBaggageEntry(ctx, key)
}

// SetBaggageEntry returns ctx with the baggage member key set to value (Node
// setBaggageEntry). The baggage travels with every Call made with the returned ctx
// and is copied onto the spans started under it. An invalid key is an error.
func SetBaggageEntry(ctx context.Context, key, value string) (context.Context, error) {
	return telemetry.SetBaggageEntry(ctx, key, value)
}

// RemoveBaggageEntry returns ctx without the baggage member key (Node
// removeBaggageEntry).
func RemoveBaggageEntry(ctx context.Context, key string) context.Context {
	return telemetry.RemoveBaggageEntry(ctx, key)
}

// GetAllBaggage returns every baggage member in ctx as a key/value map (Node
// getAllBaggage).
func GetAllBaggage(ctx context.Context) map[string]string {
	return telemetry.GetAllBaggage(ctx)
}
