package telemetry

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"log"
	"time"

	"go.opentelemetry.io/otel/attribute"
	otellog "go.opentelemetry.io/otel/log"
)

// Log severities used by the Logger (Node SeverityNumber DEBUG, INFO, WARN, ERROR).
const (
	SeverityDebug = otellog.SeverityDebug
	SeverityInfo  = otellog.SeverityInfo
	SeverityWarn  = otellog.SeverityWarn
	SeverityError = otellog.SeverityError
)

// LogLogger returns the OpenTelemetry logger of the active System, or nil (Node
// getLogger).
func LogLogger() otellog.Logger {
	if s := Active(); s != nil {
		return s.logger
	}
	return nil
}

// EmitLog is the body of Node Logger.emit: it attaches trace_id and span_id (explicit,
// else from the span in ctx), service.name when set and log.data when data is not nil,
// then emits a record with severity and body message on the active System. Without one
// it falls back to the standard logger, as Node falls back to console.
func EmitLog(ctx context.Context, severity otellog.Severity, message string, traceID, spanID, serviceName string, data any) {
	if ctx == nil {
		ctx = context.Background()
	}
	logger := LogLogger()
	if logger == nil {
		if data != nil {
			log.Println(message, SafeStringify(data))
		} else {
			log.Println(message)
		}
		return
	}
	if traceID == "" {
		traceID = CurrentTraceID(ctx)
	}
	if spanID == "" {
		spanID = CurrentSpanID(ctx)
	}
	var attrs []attribute.KeyValue
	if traceID != "" {
		attrs = append(attrs, attribute.String("trace_id", traceID))
	}
	if spanID != "" {
		attrs = append(attrs, attribute.String("span_id", spanID))
	}
	if serviceName != "" {
		attrs = append(attrs, attribute.String("service.name", serviceName))
	}
	if data != nil {
		attrs = append(attrs, attribute.KeyValue{Key: "log.data", Value: ToLogValue(data)})
	}
	var rec otellog.Record
	rec.SetTimestamp(time.Now())
	rec.SetSeverity(severity)
	rec.SetBody(attribute.StringValue(message))
	rec.AddAttributes(attrs...)
	logger.Emit(ctx, rec)
}

// ToLogValue converts a Go value to an OpenTelemetry log value the way the Node logger
// hands its data to the OTel API: strings, booleans and numbers stay scalars, []byte
// becomes bytes, and anything else goes through its JSON form (objects become key/value
// lists in field order, arrays become arrays, integral numbers integers).
func ToLogValue(v any) attribute.Value {
	switch x := v.(type) {
	case nil:
		return attribute.Value{}
	case attribute.Value:
		return x
	case string:
		return attribute.StringValue(x)
	case bool:
		return attribute.BoolValue(x)
	case int:
		return attribute.IntValue(x)
	case int8:
		return attribute.Int64Value(int64(x))
	case int16:
		return attribute.Int64Value(int64(x))
	case int32:
		return attribute.Int64Value(int64(x))
	case int64:
		return attribute.Int64Value(x)
	case uint8:
		return attribute.Int64Value(int64(x))
	case uint16:
		return attribute.Int64Value(int64(x))
	case uint32:
		return attribute.Int64Value(int64(x))
	case float32:
		return attribute.Float64Value(float64(x))
	case float64:
		return attribute.Float64Value(x)
	case []byte:
		return attribute.ByteSliceValue(x)
	}
	raw, err := marshalJSON(v)
	if err != nil {
		return attribute.StringValue(fmt.Sprint(v))
	}
	dec := json.NewDecoder(bytes.NewReader(raw))
	dec.UseNumber()
	val, err := decodeLogValue(dec)
	if err != nil {
		return attribute.StringValue(string(raw))
	}
	return val
}

func decodeLogValue(dec *json.Decoder) (attribute.Value, error) {
	tok, err := dec.Token()
	if err != nil {
		return attribute.Value{}, err
	}
	switch t := tok.(type) {
	case json.Delim:
		if t == '{' {
			var kvs []attribute.KeyValue
			for dec.More() {
				keyTok, err := dec.Token()
				if err != nil {
					return attribute.Value{}, err
				}
				key, _ := keyTok.(string)
				val, err := decodeLogValue(dec)
				if err != nil {
					return attribute.Value{}, err
				}
				kvs = append(kvs, attribute.KeyValue{Key: attribute.Key(key), Value: val})
			}
			_, err := dec.Token()
			return attribute.MapValue(kvs...), err
		}
		var vals []attribute.Value
		for dec.More() {
			val, err := decodeLogValue(dec)
			if err != nil {
				return attribute.Value{}, err
			}
			vals = append(vals, val)
		}
		_, err := dec.Token()
		return attribute.SliceValue(vals...), err
	case string:
		return attribute.StringValue(t), nil
	case json.Number:
		if i, err := t.Int64(); err == nil {
			return attribute.Int64Value(i), nil
		}
		f, err := t.Float64()
		return attribute.Float64Value(f), err
	case bool:
		return attribute.BoolValue(t), nil
	default:
		return attribute.Value{}, nil
	}
}
