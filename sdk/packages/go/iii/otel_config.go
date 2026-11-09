package iii

import "time"

// OtelConfig configures the OpenTelemetry pipeline a [Client] runs: spans, metrics and
// logs exported to the engine over a dedicated WebSocket at <engine address>/otel, the
// "execute <function id>" span around every handler, and the worker resource gauges. It
// mirrors the Node SDK's OtelConfig (the otel option of registerWorker) field for
// field. The engine URL and the handshake headers are not part of it: the client uses
// its own address and its [WithHeaders] headers (plus the III_TOKEN bearer), as Node
// does for engineWsUrl and engineWsHeaders.
//
// Telemetry is enabled by default, as in the Node SDK: a client started without
// [WithOtel] exports unless OTEL_ENABLED is "false", "0", "no" or "off". A zero field
// means "not set": the environment variable named on the field, then the default,
// fills it in.
//
// OpenTelemetry globals: when telemetry starts, the client installs its tracer
// provider, its meter provider and the W3C Trace Context + Baggage propagator as the
// OpenTelemetry globals (otel.Tracer, otel.Meter, otel.GetTextMapPropagator), each only
// while that global is still the otel default. A provider or propagator the
// application installed earlier, or another client installed first, is never replaced:
// first registration wins, as in the Node SDK. [Client.Close] puts the default back
// into every global that still holds this client's. The client's invocation spans and
// worker gauges, and the observability helpers (which use the client started last),
// always use the client's own providers, so they reach the engine either way. Spans the
// application creates with its own provider are parented to the invocation span
// through ctx and exported wherever that provider exports. Call and inbound
// invocations always propagate W3C Trace Context and Baggage, whatever the global
// propagator.
type OtelConfig struct {
	// Enabled turns telemetry export on or off. Nil reads OTEL_ENABLED
	// ("false", "0", "no" or "off" disable it); the default is true.
	Enabled *bool
	// ServiceName is the service.name resource attribute and the instrumentation scope
	// of the SDK's tracer, meter and logger. Empty reads OTEL_SERVICE_NAME; the default
	// is "iii-go".
	ServiceName string
	// ServiceVersion is the service.version resource attribute. Empty reads
	// SERVICE_VERSION; the default is "unknown".
	ServiceVersion string
	// ServiceNamespace is the service.namespace resource attribute, omitted when empty.
	// Empty reads SERVICE_NAMESPACE.
	ServiceNamespace string
	// ServiceInstanceID is the service.instance.id resource attribute. Empty reads
	// SERVICE_INSTANCE_ID; the default is a random UUID.
	ServiceInstanceID string
	// MetricsEnabled turns metrics export on or off. Nil reads OTEL_METRICS_ENABLED
	// ("false", "0", "no" or "off" disable it); the default is true.
	MetricsEnabled *bool
	// MetricsExportInterval is the period of the metrics export. Zero means 60 seconds.
	MetricsExportInterval time.Duration
	// SpansFlushInterval is how long an ended span waits in the batch before it is sent.
	// Zero reads OTEL_SPANS_FLUSH_INTERVAL_MS (milliseconds); the default is 100ms.
	SpansFlushInterval time.Duration
	// LogsFlushInterval is how long a log record waits in the batch before it is sent.
	// Zero reads OTEL_LOGS_FLUSH_INTERVAL_MS (milliseconds); the default is 100ms.
	LogsFlushInterval time.Duration
	// LogsBatchSize is the maximum number of log records per export. Zero reads
	// OTEL_LOGS_BATCH_SIZE; the default is 1.
	LogsBatchSize int
	// FetchInstrumentationEnabled turns the HTTP client spans of
	// observability.NewTransport and observability.NewHTTPClient on or off (the Go
	// counterpart of Node's fetch instrumentation). Nil means true.
	FetchInstrumentationEnabled *bool
	// ReconnectionConfig is the reconnect schedule of the telemetry WebSocket. Nil means
	// [DefaultReconnectConfig]; a non-nil value is used as given.
	ReconnectionConfig *ReconnectConfig
}

// WithOtel configures the client's OpenTelemetry pipeline (the otel option of Node
// registerWorker). Telemetry starts with the connection lifecycle ([RegisterWorker] or
// [Client.Connect]) and [Client.Close] flushes and stops it. Without this option the
// defaults documented on [OtelConfig] apply, and they enable telemetry.
//
//	c := iii.RegisterWorker(url, iii.WithOtel(iii.OtelConfig{ServiceName: "orders"}))
//
// To turn telemetry off:
//
//	off := false
//	c := iii.RegisterWorker(url, iii.WithOtel(iii.OtelConfig{Enabled: &off}))
func WithOtel(cfg OtelConfig) Option {
	return func(c *Client) {
		c.otel.config = cfg
	}
}

// WithMetricsReporting turns the worker resource gauges on or off (Node
// enableMetricsReporting): the iii.worker.memory.*, iii.worker.cpu.*,
// iii.worker.event_loop.lag_ms and iii.worker.uptime_seconds metrics, tagged with
// worker.id and worker.name. The default is true. The gauges start when the engine
// acknowledges the worker and stop when the connection drops; they need telemetry and
// its metrics to be enabled (see [OtelConfig]).
func WithMetricsReporting(enabled bool) Option {
	return func(c *Client) {
		c.otel.metricsReportingDisabled = !enabled
	}
}
