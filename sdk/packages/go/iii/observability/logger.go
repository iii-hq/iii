package observability

import (
	"context"

	"github.com/iii-hq/iii/sdk/packages/go/iii/internal/telemetry"
)

// Logger is a structured logger that emits OpenTelemetry log records (Node Logger).
//
// Every call captures the trace and span of ctx, so log lines correlate with the
// distributed trace without manual wiring; TraceID and SpanID override them when set.
// Pass structured data (a map or struct) instead of formatting it into the message, so
// it can be filtered and aggregated: it is attached as the log.data attribute. When no
// client's telemetry is running, Logger falls back to the standard log package.
//
// The zero value is ready to use.
//
//	var logger observability.Logger
//
//	func handler(ctx context.Context, data json.RawMessage) (any, error) {
//	    logger.Info(ctx, "Order processed", map[string]any{"orderId": "ord_123", "amount": 49.99})
//	    ...
//	}
type Logger struct {
	// TraceID, when set, is reported as the trace_id attribute instead of the trace of
	// the call's ctx.
	TraceID string
	// ServiceName, when set, is reported as the service.name attribute.
	ServiceName string
	// SpanID, when set, is reported as the span_id attribute instead of the span of the
	// call's ctx.
	SpanID string
}

// Info logs an info-level message with optional structured data (nil for none).
func (l Logger) Info(ctx context.Context, message string, data any) {
	telemetry.EmitLog(ctx, telemetry.SeverityInfo, message, l.TraceID, l.SpanID, l.ServiceName, data)
}

// Warn logs a warning-level message with optional structured data (nil for none).
func (l Logger) Warn(ctx context.Context, message string, data any) {
	telemetry.EmitLog(ctx, telemetry.SeverityWarn, message, l.TraceID, l.SpanID, l.ServiceName, data)
}

// Error logs an error-level message with optional structured data (nil for none).
func (l Logger) Error(ctx context.Context, message string, data any) {
	telemetry.EmitLog(ctx, telemetry.SeverityError, message, l.TraceID, l.SpanID, l.ServiceName, data)
}

// Debug logs a debug-level message with optional structured data (nil for none).
func (l Logger) Debug(ctx context.Context, message string, data any) {
	telemetry.EmitLog(ctx, telemetry.SeverityDebug, message, l.TraceID, l.SpanID, l.ServiceName, data)
}
