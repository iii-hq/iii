package telemetry

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"log"
	"time"

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
	var attrs []otellog.KeyValue
	if traceID != "" {
		attrs = append(attrs, otellog.String("trace_id", traceID))
	}
	if spanID != "" {
		attrs = append(attrs, otellog.String("span_id", spanID))
	}
	if serviceName != "" {
		attrs = append(attrs, otellog.String("service.name", serviceName))
	}
	if data != nil {
		attrs = append(attrs, otellog.KeyValue{Key: "log.data", Value: ToLogValue(data)})
	}
	var rec otellog.Record
	rec.SetTimestamp(time.Now())
	rec.SetSeverity(severity)
	rec.SetBody(otellog.StringValue(message))
	rec.AddAttributes(attrs...)
	logger.Emit(ctx, rec)
}

// ToLogValue converts a Go value to an OpenTelemetry log value the way the Node logger
// hands its data to the OTel API: strings, booleans and numbers stay scalars, []byte
// becomes bytes, and anything else goes through its JSON form (objects become key/value
// lists in field order, arrays become arrays, integral numbers integers).
func ToLogValue(v any) otellog.Value {
	switch x := v.(type) {
	case nil:
		return otellog.Value{}
	case otellog.Value:
		return x
	case string:
		return otellog.StringValue(x)
	case bool:
		return otellog.BoolValue(x)
	case int:
		return otellog.IntValue(x)
	case int8:
		return otellog.Int64Value(int64(x))
	case int16:
		return otellog.Int64Value(int64(x))
	case int32:
		return otellog.Int64Value(int64(x))
	case int64:
		return otellog.Int64Value(x)
	case uint8:
		return otellog.Int64Value(int64(x))
	case uint16:
		return otellog.Int64Value(int64(x))
	case uint32:
		return otellog.Int64Value(int64(x))
	case float32:
		return otellog.Float64Value(float64(x))
	case float64:
		return otellog.Float64Value(x)
	case []byte:
		return otellog.BytesValue(x)
	}
	raw, err := marshalJSON(v)
	if err != nil {
		return otellog.StringValue(fmt.Sprint(v))
	}
	dec := json.NewDecoder(bytes.NewReader(raw))
	dec.UseNumber()
	val, err := decodeLogValue(dec)
	if err != nil {
		return otellog.StringValue(string(raw))
	}
	return val
}

func decodeLogValue(dec *json.Decoder) (otellog.Value, error) {
	tok, err := dec.Token()
	if err != nil {
		return otellog.Value{}, err
	}
	switch t := tok.(type) {
	case json.Delim:
		if t == '{' {
			var kvs []otellog.KeyValue
			for dec.More() {
				keyTok, err := dec.Token()
				if err != nil {
					return otellog.Value{}, err
				}
				key, _ := keyTok.(string)
				val, err := decodeLogValue(dec)
				if err != nil {
					return otellog.Value{}, err
				}
				kvs = append(kvs, otellog.KeyValue{Key: key, Value: val})
			}
			_, err := dec.Token()
			return otellog.MapValue(kvs...), err
		}
		var vals []otellog.Value
		for dec.More() {
			val, err := decodeLogValue(dec)
			if err != nil {
				return otellog.Value{}, err
			}
			vals = append(vals, val)
		}
		_, err := dec.Token()
		return otellog.SliceValue(vals...), err
	case string:
		return otellog.StringValue(t), nil
	case json.Number:
		if i, err := t.Int64(); err == nil {
			return otellog.Int64Value(i), nil
		}
		f, err := t.Float64()
		return otellog.Float64Value(f), err
	case bool:
		return otellog.BoolValue(t), nil
	default:
		return otellog.Value{}, nil
	}
}
