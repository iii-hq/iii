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
	"math"

	"go.opentelemetry.io/otel/log"
)

// maxInt64AsFloat is the largest int64 representable as a float64 without rounding past
// the int64 range. float64 can't represent math.MaxInt64 exactly, so we compare against
// 2^63 and treat the boundary conservatively.
const maxInt64AsFloat = float64(math.MaxInt64)

// uintToLogValue maps an unsigned integer to a log.Value without the wrap-around that
// int64(t) would cause for values above math.MaxInt64. OTel's log.Value has no uint64
// kind, so out-of-range values fall back to Float64, which preserves magnitude (with
// possible precision loss) rather than turning into a negative number.
func uintToLogValue(u uint64) log.Value {
	if u <= math.MaxInt64 {
		return log.Int64Value(int64(u))
	}
	return log.Float64Value(float64(u))
}

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
	// Native Go integer types — these reach us when a caller passes a map[string]any
	// directly to Logger (not via JSON). Map them to Int64 so they aren't stringified.
	case int:
		return log.Int64Value(int64(t))
	case int8:
		return log.Int64Value(int64(t))
	case int16:
		return log.Int64Value(int64(t))
	case int32:
		return log.Int64Value(int64(t))
	case int64:
		return log.Int64Value(t)
	case uint:
		return uintToLogValue(uint64(t))
	case uint8:
		return log.Int64Value(int64(t))
	case uint16:
		return log.Int64Value(int64(t))
	case uint32:
		return log.Int64Value(int64(t))
	case uint64:
		return uintToLogValue(t)
	case float32:
		return log.Float64Value(float64(t))
	case float64:
		// encoding/json decodes all numbers as float64. Preserve whole values as Int64,
		// matching the Rust as_i64-first behavior — but only when they fit, so a large
		// magnitude isn't silently truncated by int64(t).
		if t == math.Trunc(t) && t >= math.MinInt64 && t <= maxInt64AsFloat {
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
