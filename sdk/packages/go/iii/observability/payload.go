package observability

import "github.com/iii-hq/iii/sdk/packages/go/iii/internal/telemetry"

// RedactedPlaceholder replaces the value of a sensitive key in recorded payloads (Node
// REDACTED_PLACEHOLDER).
const RedactedPlaceholder = telemetry.RedactedPlaceholder

// Redact returns v with the value of every sensitive key, at any depth, replaced by
// [RedactedPlaceholder] (Node redact). A key is sensitive when it contains api_key,
// apikey, api-key, password, secret, credential, authorization, auth_token,
// access_token, refresh_token, bearer, private_key or client_secret (any case), or is
// "token" or ends with "_token" or "-token". A map[string]any or []any is copied
// recursively and nil and scalars are returned as they are; any other value (a struct,
// a typed map, json.RawMessage) is serialized and returned as a redacted
// json.RawMessage.
func Redact(v any) any { return telemetry.Redact(v) }

// RedactAndTruncate redacts v, serializes it to JSON and caps the result at maxBytes
// bytes, ending a cut payload with a truncation marker and never splitting a UTF-8
// sequence; maxBytes <= 0 means no cap (Node redactAndTruncate). It reports whether the
// payload was cut. A value that cannot be serialized becomes "null".
func RedactAndTruncate(v any, maxBytes int) (json string, truncated bool) {
	return telemetry.RedactAndTruncate(v, maxBytes)
}

// ResolveMaxBytesFromEnv returns the recorded-payload cap set by
// III_TRACE_PAYLOAD_MAX_BYTES, or 0 (no cap) when it is unset, blank, "unlimited", not
// a plain positive integer (Node resolveMaxBytesFromEnv, whose null is 0 here).
func ResolveMaxBytesFromEnv() int { return telemetry.ResolveMaxBytesFromEnv() }

// SafeStringify serializes v to JSON, or returns "[unserializable]" when it cannot be
// serialized (Node safeStringify; a cyclic value is reported as unserializable).
func SafeStringify(v any) string { return telemetry.SafeStringify(v) }

// OtelLogEvent is a log record as the engine reports it (Node OtelLogEvent), for
// functions that consume the engine's log stream.
type OtelLogEvent struct {
	// TimestampUnixNano is the record time in Unix nanoseconds.
	TimestampUnixNano uint64 `json:"timestamp_unix_nano"`
	// ObservedTimestampUnixNano is the time the record was observed, in Unix nanoseconds.
	ObservedTimestampUnixNano uint64 `json:"observed_timestamp_unix_nano"`
	// SeverityNumber is the OpenTelemetry severity (1-24): TRACE 1-4, DEBUG 5-8,
	// INFO 9-12, WARN 13-16, ERROR 17-20, FATAL 21-24.
	SeverityNumber int `json:"severity_number"`
	// SeverityText is the severity name (e.g. "INFO", "WARN", "ERROR").
	SeverityText string `json:"severity_text"`
	// Body is the log message.
	Body string `json:"body"`
	// Attributes are the record's structured attributes.
	Attributes map[string]any `json:"attributes"`
	// TraceID correlates the record with a trace, when known.
	TraceID string `json:"trace_id,omitempty"`
	// SpanID correlates the record with a span, when known.
	SpanID string `json:"span_id,omitempty"`
	// Resource holds the resource attributes of the emitting service.
	Resource map[string]string `json:"resource"`
	// ServiceName is the service that emitted the record.
	ServiceName string `json:"service_name"`
	// InstrumentationScopeName is the instrumentation scope name, when known.
	InstrumentationScopeName string `json:"instrumentation_scope_name,omitempty"`
	// InstrumentationScopeVersion is the instrumentation scope version, when known.
	InstrumentationScopeVersion string `json:"instrumentation_scope_version,omitempty"`
}
