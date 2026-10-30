// Package observability holds the OpenTelemetry helpers of the iii Go SDK, the
// counterpart of the Node SDK's @iii-dev/helpers/observability: a structured [Logger]
// that correlates log records with traces, a traced HTTP client ([NewTransport],
// [NewHTTPClient]), W3C trace context and baggage helpers, span helpers ([WithSpan],
// [RecordSpanEvent], ...), payload redaction, and the worker resource metrics.
//
// The pipeline itself is configured on the client: iii.WithOtel (iii.OtelConfig) and
// iii.WithMetricsReporting. While a client's telemetry runs, these helpers use it: log
// records, spans and metrics go to the engine with the client's resource attributes.
// Without one they degrade gracefully (the Logger writes to the standard logger, spans
// use the global OpenTelemetry tracer provider, a no-op unless the program installed
// one).
//
// Go has no implicit active context, so every helper that reads or changes the current
// span or baggage takes a context.Context: pass the ctx a handler receives.
package observability
