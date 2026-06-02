// Package observability provides OpenTelemetry and logging primitives shared across iii
// Go SDK workers — a structured Logger that emits OTel LogRecords correlated with the
// active trace, plus the init plumbing that wires an OTLP exporter. It mirrors
// sdk/packages/rust/observability (the iii-observability crate) and the Node
// @iii-dev/observability package.
//
// The iii Go SDK itself (github.com/iii-hq/iii/sdk/packages/go/iii) carries W3C trace
// context (traceparent/baggage) on the wire and ships a no-op OTel seam; this package is
// the real OTel wiring a worker opts into.
package observability

import (
	"encoding/json"

	"go.opentelemetry.io/otel/log"
)

// jsonToLogValue converts a decoded JSON value into an OpenTelemetry log.Value so nested
// objects and arrays are preserved as structured OTLP attributes (KeyValueList /
// Slice) rather than being stringified. Mirrors json_value_to_anyvalue in
// sdk/packages/rust/observability/src/logger.rs.
func jsonToLogValue(v any) log.Value {
	switch t := v.(type) {
	case nil:
		return log.StringValue("")
	case string:
		return log.StringValue(t)
	case bool:
		return log.BoolValue(t)
	case float64:
		// encoding/json decodes all numbers as float64. Preserve integers as Int64 when
		// the value is whole, matching the Rust as_i64-first behavior.
		if t == float64(int64(t)) {
			return log.Int64Value(int64(t))
		}
		return log.Float64Value(t)
	case json.Number:
		if i, err := t.Int64(); err == nil {
			return log.Int64Value(i)
		}
		if f, err := t.Float64(); err == nil {
			return log.Float64Value(f)
		}
		return log.StringValue(t.String())
	case []any:
		vals := make([]log.Value, len(t))
		for i, e := range t {
			vals[i] = jsonToLogValue(e)
		}
		return log.SliceValue(vals...)
	case map[string]any:
		kvs := make([]log.KeyValue, 0, len(t))
		for k, e := range t {
			kvs = append(kvs, log.KeyValue{Key: k, Value: jsonToLogValue(e)})
		}
		return log.MapValue(kvs...)
	default:
		// Fall back to a JSON string for anything unexpected.
		b, err := json.Marshal(t)
		if err != nil {
			return log.StringValue("")
		}
		return log.StringValue(string(b))
	}
}
