package observability

import (
	"context"
	"log/slog"
	"time"

	otellog "go.opentelemetry.io/otel/log"
	"go.opentelemetry.io/otel/trace"
)

// instrumentationName is the OTel instrumentation scope for logs emitted by this SDK.
const instrumentationName = "iii-go-sdk"

// Logger is a structured logger that emits logs as OpenTelemetry LogRecords. Every call
// captures the active span's trace context, correlating logs with distributed traces
// without manual wiring. When OTel has not been initialized (Init not called), it falls
// back to the standard library slog.
//
// Pass structured data as the second argument — a map of key/value pairs rather than
// string interpolation — so the backend can filter and aggregate on it. Mirrors the
// Logger in sdk/packages/rust/observability/src/logger.rs.
//
//	logger := observability.NewLogger()
//	logger.Info(ctx, "Order processed", map[string]any{"order_id": "ord_123", "amount": 49.99})
type Logger struct{}

// NewLogger returns a Logger. It is cheap; create one per component or reuse freely.
func NewLogger() *Logger { return &Logger{} }

// Info logs at info severity. data may be nil.
func (l *Logger) Info(ctx context.Context, message string, data map[string]any) {
	l.log(ctx, otellog.SeverityInfo, slog.LevelInfo, message, data)
}

// Warn logs at warn severity. data may be nil.
func (l *Logger) Warn(ctx context.Context, message string, data map[string]any) {
	l.log(ctx, otellog.SeverityWarn, slog.LevelWarn, message, data)
}

// Error logs at error severity. data may be nil.
func (l *Logger) Error(ctx context.Context, message string, data map[string]any) {
	l.log(ctx, otellog.SeverityError, slog.LevelError, message, data)
}

// Debug logs at debug severity. data may be nil.
func (l *Logger) Debug(ctx context.Context, message string, data map[string]any) {
	l.log(ctx, otellog.SeverityDebug, slog.LevelDebug, message, data)
}

func (l *Logger) log(ctx context.Context, sev otellog.Severity, slogLevel slog.Level, message string, data map[string]any) {
	if l.emitOTel(ctx, sev, message, data) {
		return
	}
	// Fallback: OTel not initialized. Emit via slog with the structured data inline.
	attrs := make([]any, 0, len(data))
	for k, v := range data {
		attrs = append(attrs, slog.Any(k, v))
	}
	slog.Default().Log(ctx, slogLevel, message, attrs...)
}

// emitOTel emits a LogRecord through the installed OTel logger provider, attaching the
// active span's trace context. Returns false if no provider is installed.
func (l *Logger) emitOTel(ctx context.Context, sev otellog.Severity, message string, data map[string]any) bool {
	provider := getLoggerProvider()
	if provider == nil {
		return false
	}
	logger := provider.Logger(instrumentationName)

	var rec otellog.Record
	now := time.Now()
	rec.SetTimestamp(now)
	rec.SetObservedTimestamp(now)
	rec.SetSeverity(sev)
	rec.SetBody(otellog.StringValue(message))
	if len(data) > 0 {
		rec.AddAttributes(otellog.KeyValue{Key: "log.data", Value: jsonToLogValue(mapToAny(data))})
	}

	// The OTel Go log bridge reads trace context from ctx automatically on Emit, so the
	// record is correlated with the active span (trace.SpanContextFromContext below is
	// for the no-span case / documentation of intent).
	_ = trace.SpanContextFromContext(ctx)

	logger.Emit(ctx, rec)
	return true
}

// mapToAny adapts a map[string]any to the any-typed walker jsonToLogValue expects.
func mapToAny(m map[string]any) any { return map[string]any(m) }
