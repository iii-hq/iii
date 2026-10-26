package telemetry

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"math"
	"strconv"
	"time"

	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/codes"
	otellog "go.opentelemetry.io/otel/log"
	"go.opentelemetry.io/otel/sdk/instrumentation"
	sdklog "go.opentelemetry.io/otel/sdk/log"
	"go.opentelemetry.io/otel/sdk/metric/metricdata"
	"go.opentelemetry.io/otel/sdk/resource"
	sdktrace "go.opentelemetry.io/otel/sdk/trace"
	"go.opentelemetry.io/otel/trace"
)

// This file serializes spans, log records and metrics into the OTLP/JSON documents the
// engine ingests (engine/src/workers/observability/otel.rs), shaped exactly like the
// Node SDK's @opentelemetry/otlp-transformer JsonTraceSerializer, JsonLogsSerializer and
// JsonMetricsSerializer (JSON_ENCODER): camelCase fields, lowercase-hex trace and span
// ids, nanosecond timestamps as decimal strings, enums as numbers, integer attributes
// as JSON numbers. Fields the Node serializer leaves undefined are omitted.

// Span flag bits (OTLP SpanFlags), as otlp-transformer buildSpanFlagsFrom sets them.
const (
	spanFlagsContextHasIsRemote = 0x100
	spanFlagsContextIsRemote    = 0x200
)

// OTLP AggregationTemporality values. Go's metricdata enum numbers them differently.
const (
	otlpTemporalityDelta      = 1
	otlpTemporalityCumulative = 2
)

// --- common ---------------------------------------------------------------------

type jsonKeyValue struct {
	Key   string       `json:"key"`
	Value jsonAnyValue `json:"value"`
}

type jsonAnyValue struct {
	StringValue *string          `json:"stringValue,omitempty"`
	BoolValue   *bool            `json:"boolValue,omitempty"`
	IntValue    *int64           `json:"intValue,omitempty"`
	DoubleValue *jsonFloat       `json:"doubleValue,omitempty"`
	BytesValue  *string          `json:"bytesValue,omitempty"`
	ArrayValue  *jsonArrayValue  `json:"arrayValue,omitempty"`
	KvlistValue *jsonKvlistValue `json:"kvlistValue,omitempty"`
}

type jsonArrayValue struct {
	Values []jsonAnyValue `json:"values"`
}

type jsonKvlistValue struct {
	Values []jsonKeyValue `json:"values"`
}

// jsonFloat encodes like JSON.stringify: NaN and the infinities become null.
type jsonFloat float64

func (f jsonFloat) MarshalJSON() ([]byte, error) {
	v := float64(f)
	if math.IsNaN(v) || math.IsInf(v, 0) {
		return []byte("null"), nil
	}
	return json.Marshal(v)
}

func floatPtr(v float64) *jsonFloat {
	f := jsonFloat(v)
	return &f
}

type jsonResource struct {
	Attributes             []jsonKeyValue `json:"attributes"`
	DroppedAttributesCount int            `json:"droppedAttributesCount"`
	SchemaURL              string         `json:"schemaUrl,omitempty"`
}

type jsonScope struct {
	Name                   string         `json:"name"`
	Version                string         `json:"version,omitempty"`
	Attributes             []jsonKeyValue `json:"attributes,omitempty"`
	DroppedAttributesCount *int           `json:"droppedAttributesCount,omitempty"`
}

func toJSONResource(res *resource.Resource) jsonResource {
	out := jsonResource{Attributes: []jsonKeyValue{}}
	if res == nil {
		return out
	}
	out.Attributes = toJSONAttributes(res.Attributes())
	out.SchemaURL = res.SchemaURL()
	return out
}

func toJSONScope(s instrumentation.Scope) jsonScope {
	out := jsonScope{Name: s.Name, Version: s.Version}
	if s.Attributes.Len() > 0 {
		out.Attributes = toJSONAttributes(s.Attributes.ToSlice())
		zero := 0
		out.DroppedAttributesCount = &zero
	}
	return out
}

func scopeKey(s instrumentation.Scope) string {
	return s.Name + "@" + s.Version + ":" + s.SchemaURL + "|" + s.Attributes.Encoded(attribute.DefaultEncoder())
}

func toJSONAttributes(attrs []attribute.KeyValue) []jsonKeyValue {
	out := make([]jsonKeyValue, 0, len(attrs))
	for _, kv := range attrs {
		out = append(out, jsonKeyValue{Key: string(kv.Key), Value: toJSONAttrValue(kv.Value)})
	}
	return out
}

func toJSONAttrValue(v attribute.Value) jsonAnyValue {
	switch v.Type() {
	case attribute.BOOL:
		b := v.AsBool()
		return jsonAnyValue{BoolValue: &b}
	case attribute.INT64:
		i := v.AsInt64()
		return jsonAnyValue{IntValue: &i}
	case attribute.FLOAT64:
		return jsonAnyValue{DoubleValue: floatPtr(v.AsFloat64())}
	case attribute.STRING:
		s := v.AsString()
		return jsonAnyValue{StringValue: &s}
	case attribute.BOOLSLICE:
		vals := v.AsBoolSlice()
		out := make([]jsonAnyValue, len(vals))
		for i := range vals {
			out[i] = jsonAnyValue{BoolValue: &vals[i]}
		}
		return jsonAnyValue{ArrayValue: &jsonArrayValue{Values: out}}
	case attribute.INT64SLICE:
		vals := v.AsInt64Slice()
		out := make([]jsonAnyValue, len(vals))
		for i := range vals {
			out[i] = jsonAnyValue{IntValue: &vals[i]}
		}
		return jsonAnyValue{ArrayValue: &jsonArrayValue{Values: out}}
	case attribute.FLOAT64SLICE:
		vals := v.AsFloat64Slice()
		out := make([]jsonAnyValue, len(vals))
		for i := range vals {
			out[i] = jsonAnyValue{DoubleValue: floatPtr(vals[i])}
		}
		return jsonAnyValue{ArrayValue: &jsonArrayValue{Values: out}}
	case attribute.STRINGSLICE:
		vals := v.AsStringSlice()
		out := make([]jsonAnyValue, len(vals))
		for i := range vals {
			out[i] = jsonAnyValue{StringValue: &vals[i]}
		}
		return jsonAnyValue{ArrayValue: &jsonArrayValue{Values: out}}
	default:
		return jsonAnyValue{}
	}
}

// nanos encodes t like otlp-transformer encodeAsString; the zero time is "0".
func nanos(t time.Time) string {
	if t.IsZero() || t.UnixNano() < 0 {
		return "0"
	}
	return strconv.FormatInt(t.UnixNano(), 10)
}

// marshalJSON encodes v without HTML escaping, as JSON.stringify does.
func marshalJSON(v any) ([]byte, error) {
	var buf bytes.Buffer
	enc := json.NewEncoder(&buf)
	enc.SetEscapeHTML(false)
	if err := enc.Encode(v); err != nil {
		return nil, err
	}
	return bytes.TrimSuffix(buf.Bytes(), []byte("\n")), nil
}

// --- traces ---------------------------------------------------------------------

type jsonTraceRequest struct {
	ResourceSpans []jsonResourceSpans `json:"resourceSpans"`
}

type jsonResourceSpans struct {
	Resource   jsonResource     `json:"resource"`
	ScopeSpans []jsonScopeSpans `json:"scopeSpans"`
	SchemaURL  string           `json:"schemaUrl,omitempty"`
}

type jsonScopeSpans struct {
	Scope     jsonScope  `json:"scope"`
	Spans     []jsonSpan `json:"spans"`
	SchemaURL string     `json:"schemaUrl,omitempty"`
}

type jsonSpan struct {
	TraceID                string         `json:"traceId"`
	SpanID                 string         `json:"spanId"`
	ParentSpanID           string         `json:"parentSpanId,omitempty"`
	TraceState             string         `json:"traceState,omitempty"`
	Name                   string         `json:"name"`
	Kind                   int            `json:"kind"`
	StartTimeUnixNano      string         `json:"startTimeUnixNano"`
	EndTimeUnixNano        string         `json:"endTimeUnixNano"`
	Attributes             []jsonKeyValue `json:"attributes"`
	DroppedAttributesCount int            `json:"droppedAttributesCount"`
	Events                 []jsonEvent    `json:"events"`
	DroppedEventsCount     int            `json:"droppedEventsCount"`
	Status                 jsonStatus     `json:"status"`
	Links                  []jsonLink     `json:"links"`
	DroppedLinksCount      int            `json:"droppedLinksCount"`
	Flags                  uint32         `json:"flags"`
}

type jsonEvent struct {
	Attributes             []jsonKeyValue `json:"attributes"`
	Name                   string         `json:"name"`
	TimeUnixNano           string         `json:"timeUnixNano"`
	DroppedAttributesCount int            `json:"droppedAttributesCount"`
}

type jsonStatus struct {
	Code    int    `json:"code"`
	Message string `json:"message,omitempty"`
}

type jsonLink struct {
	Attributes             []jsonKeyValue `json:"attributes"`
	SpanID                 string         `json:"spanId"`
	TraceID                string         `json:"traceId"`
	TraceState             string         `json:"traceState,omitempty"`
	DroppedAttributesCount int            `json:"droppedAttributesCount"`
	Flags                  uint32         `json:"flags"`
}

// otlpStatusCode maps Go's codes (Unset 0, Error 1, Ok 2) to OTLP's (Unset 0, Ok 1,
// Error 2), the numbers the Node serializer emits.
func otlpStatusCode(c codes.Code) int {
	switch c {
	case codes.Ok:
		return 1
	case codes.Error:
		return 2
	default:
		return 0
	}
}

func spanFlags(flags trace.TraceFlags, remote bool) uint32 {
	out := uint32(flags) | spanFlagsContextHasIsRemote
	if remote {
		out |= spanFlagsContextIsRemote
	}
	return out
}

// MarshalTraces encodes spans as an ExportTraceServiceRequest, grouped by resource and
// then instrumentation scope in first-seen order (otlp-transformer
// createExportTraceServiceRequest).
func MarshalTraces(spans []sdktrace.ReadOnlySpan) ([]byte, error) {
	req := jsonTraceRequest{ResourceSpans: []jsonResourceSpans{}}
	resIndex := map[*resource.Resource]int{}
	scopeIndex := map[*resource.Resource]map[string]int{}
	for _, s := range spans {
		res := s.Resource()
		ri, ok := resIndex[res]
		if !ok {
			r := toJSONResource(res)
			req.ResourceSpans = append(req.ResourceSpans, jsonResourceSpans{Resource: r, ScopeSpans: []jsonScopeSpans{}, SchemaURL: r.SchemaURL})
			ri = len(req.ResourceSpans) - 1
			resIndex[res] = ri
			scopeIndex[res] = map[string]int{}
		}
		scope := s.InstrumentationScope()
		key := scopeKey(scope)
		si, ok := scopeIndex[res][key]
		if !ok {
			req.ResourceSpans[ri].ScopeSpans = append(req.ResourceSpans[ri].ScopeSpans, jsonScopeSpans{Scope: toJSONScope(scope), Spans: []jsonSpan{}, SchemaURL: scope.SchemaURL})
			si = len(req.ResourceSpans[ri].ScopeSpans) - 1
			scopeIndex[res][key] = si
		}
		ss := &req.ResourceSpans[ri].ScopeSpans[si]
		ss.Spans = append(ss.Spans, toJSONSpan(s))
	}
	return marshalJSON(req)
}

func toJSONSpan(s sdktrace.ReadOnlySpan) jsonSpan {
	sc := s.SpanContext()
	parent := s.Parent()
	out := jsonSpan{
		TraceID:                sc.TraceID().String(),
		SpanID:                 sc.SpanID().String(),
		TraceState:             sc.TraceState().String(),
		Name:                   s.Name(),
		Kind:                   int(s.SpanKind()),
		StartTimeUnixNano:      nanos(s.StartTime()),
		EndTimeUnixNano:        nanos(s.EndTime()),
		Attributes:             toJSONAttributes(s.Attributes()),
		DroppedAttributesCount: s.DroppedAttributes(),
		Events:                 []jsonEvent{},
		DroppedEventsCount:     s.DroppedEvents(),
		Status:                 jsonStatus{Code: otlpStatusCode(s.Status().Code), Message: s.Status().Description},
		Links:                  []jsonLink{},
		DroppedLinksCount:      s.DroppedLinks(),
		Flags:                  spanFlags(sc.TraceFlags(), parent.IsRemote()),
	}
	if parent.SpanID().IsValid() {
		out.ParentSpanID = parent.SpanID().String()
	}
	for _, e := range s.Events() {
		out.Events = append(out.Events, jsonEvent{
			Attributes:             toJSONAttributes(e.Attributes),
			Name:                   e.Name,
			TimeUnixNano:           nanos(e.Time),
			DroppedAttributesCount: e.DroppedAttributeCount,
		})
	}
	for _, l := range s.Links() {
		out.Links = append(out.Links, jsonLink{
			Attributes:             toJSONAttributes(l.Attributes),
			SpanID:                 l.SpanContext.SpanID().String(),
			TraceID:                l.SpanContext.TraceID().String(),
			TraceState:             l.SpanContext.TraceState().String(),
			DroppedAttributesCount: l.DroppedAttributeCount,
			Flags:                  spanFlags(l.SpanContext.TraceFlags(), l.SpanContext.IsRemote()),
		})
	}
	return out
}

// --- logs -----------------------------------------------------------------------

type jsonLogsRequest struct {
	ResourceLogs []jsonResourceLogs `json:"resourceLogs"`
}

type jsonResourceLogs struct {
	Resource  jsonResource    `json:"resource"`
	ScopeLogs []jsonScopeLogs `json:"scopeLogs"`
	SchemaURL string          `json:"schemaUrl,omitempty"`
}

type jsonScopeLogs struct {
	Scope      jsonScope       `json:"scope"`
	LogRecords []jsonLogRecord `json:"logRecords"`
	SchemaURL  string          `json:"schemaUrl,omitempty"`
}

type jsonLogRecord struct {
	TimeUnixNano           string         `json:"timeUnixNano"`
	ObservedTimeUnixNano   string         `json:"observedTimeUnixNano"`
	SeverityNumber         int            `json:"severityNumber,omitempty"`
	SeverityText           string         `json:"severityText,omitempty"`
	Body                   jsonAnyValue   `json:"body"`
	EventName              string         `json:"eventName,omitempty"`
	Attributes             []jsonKeyValue `json:"attributes"`
	DroppedAttributesCount int            `json:"droppedAttributesCount"`
	Flags                  *uint32        `json:"flags,omitempty"`
	TraceID                string         `json:"traceId,omitempty"`
	SpanID                 string         `json:"spanId,omitempty"`
}

// MarshalLogs encodes records as an ExportLogsServiceRequest, grouped by resource and
// instrumentation scope (otlp-transformer createExportLogsServiceRequest).
func MarshalLogs(records []sdklog.Record) ([]byte, error) {
	req := jsonLogsRequest{ResourceLogs: []jsonResourceLogs{}}
	resIndex := map[*resource.Resource]int{}
	scopeIndex := map[*resource.Resource]map[string]int{}
	for i := range records {
		r := &records[i]
		res := r.Resource()
		ri, ok := resIndex[res]
		if !ok {
			jr := toJSONResource(res)
			req.ResourceLogs = append(req.ResourceLogs, jsonResourceLogs{Resource: jr, ScopeLogs: []jsonScopeLogs{}, SchemaURL: jr.SchemaURL})
			ri = len(req.ResourceLogs) - 1
			resIndex[res] = ri
			scopeIndex[res] = map[string]int{}
		}
		scope := r.InstrumentationScope()
		key := scopeKey(scope)
		si, ok := scopeIndex[res][key]
		if !ok {
			req.ResourceLogs[ri].ScopeLogs = append(req.ResourceLogs[ri].ScopeLogs, jsonScopeLogs{Scope: toJSONScope(scope), LogRecords: []jsonLogRecord{}, SchemaURL: scope.SchemaURL})
			si = len(req.ResourceLogs[ri].ScopeLogs) - 1
			scopeIndex[res][key] = si
		}
		sl := &req.ResourceLogs[ri].ScopeLogs[si]
		sl.LogRecords = append(sl.LogRecords, toJSONLogRecord(r))
	}
	return marshalJSON(req)
}

func toJSONLogRecord(r *sdklog.Record) jsonLogRecord {
	ts := r.Timestamp()
	if ts.IsZero() {
		// The Node SDK always stamps a record (timestamp ?? Date.now()); the Go SDK only
		// guarantees the observed time.
		ts = r.ObservedTimestamp()
	}
	out := jsonLogRecord{
		TimeUnixNano:           nanos(ts),
		ObservedTimeUnixNano:   nanos(r.ObservedTimestamp()),
		SeverityNumber:         int(r.Severity()),
		SeverityText:           r.SeverityText(),
		Body:                   toJSONLogValue(r.Body()),
		EventName:              r.EventName(),
		Attributes:             make([]jsonKeyValue, 0, r.AttributesLen()),
		DroppedAttributesCount: r.DroppedAttributes(),
	}
	r.WalkAttributes(func(kv otellog.KeyValue) bool {
		out.Attributes = append(out.Attributes, jsonKeyValue{Key: kv.Key, Value: toJSONLogValue(kv.Value)})
		return true
	})
	if r.TraceID().IsValid() {
		flags := uint32(r.TraceFlags())
		out.Flags = &flags
		out.TraceID = r.TraceID().String()
		out.SpanID = r.SpanID().String()
	}
	return out
}

func toJSONLogValue(v otellog.Value) jsonAnyValue {
	switch v.Kind() {
	case otellog.KindBool:
		b := v.AsBool()
		return jsonAnyValue{BoolValue: &b}
	case otellog.KindInt64:
		i := v.AsInt64()
		return jsonAnyValue{IntValue: &i}
	case otellog.KindFloat64:
		return jsonAnyValue{DoubleValue: floatPtr(v.AsFloat64())}
	case otellog.KindString:
		s := v.AsString()
		return jsonAnyValue{StringValue: &s}
	case otellog.KindBytes:
		s := base64.StdEncoding.EncodeToString(v.AsBytes())
		return jsonAnyValue{BytesValue: &s}
	case otellog.KindSlice:
		vals := v.AsSlice()
		out := make([]jsonAnyValue, len(vals))
		for i, e := range vals {
			out[i] = toJSONLogValue(e)
		}
		return jsonAnyValue{ArrayValue: &jsonArrayValue{Values: out}}
	case otellog.KindMap:
		kvs := v.AsMap()
		out := make([]jsonKeyValue, len(kvs))
		for i, kv := range kvs {
			out[i] = jsonKeyValue{Key: kv.Key, Value: toJSONLogValue(kv.Value)}
		}
		return jsonAnyValue{KvlistValue: &jsonKvlistValue{Values: out}}
	default:
		return jsonAnyValue{}
	}
}

// --- metrics --------------------------------------------------------------------

type jsonMetricsRequest struct {
	ResourceMetrics []jsonResourceMetrics `json:"resourceMetrics"`
}

type jsonResourceMetrics struct {
	Resource     jsonResource       `json:"resource"`
	SchemaURL    string             `json:"schemaUrl,omitempty"`
	ScopeMetrics []jsonScopeMetrics `json:"scopeMetrics"`
}

type jsonScopeMetrics struct {
	Scope     jsonScope    `json:"scope"`
	Metrics   []jsonMetric `json:"metrics"`
	SchemaURL string       `json:"schemaUrl,omitempty"`
}

type jsonMetric struct {
	Name                 string                    `json:"name"`
	Description          string                    `json:"description"`
	Unit                 string                    `json:"unit"`
	Sum                  *jsonSum                  `json:"sum,omitempty"`
	Gauge                *jsonGauge                `json:"gauge,omitempty"`
	Histogram            *jsonHistogram            `json:"histogram,omitempty"`
	ExponentialHistogram *jsonExponentialHistogram `json:"exponentialHistogram,omitempty"`
}

type jsonSum struct {
	AggregationTemporality int               `json:"aggregationTemporality"`
	IsMonotonic            bool              `json:"isMonotonic"`
	DataPoints             []jsonNumberPoint `json:"dataPoints"`
}

type jsonGauge struct {
	DataPoints []jsonNumberPoint `json:"dataPoints"`
}

type jsonNumberPoint struct {
	Attributes        []jsonKeyValue `json:"attributes"`
	StartTimeUnixNano string         `json:"startTimeUnixNano"`
	TimeUnixNano      string         `json:"timeUnixNano"`
	AsInt             *int64         `json:"asInt,omitempty"`
	AsDouble          *jsonFloat     `json:"asDouble,omitempty"`
}

type jsonHistogram struct {
	AggregationTemporality int                  `json:"aggregationTemporality"`
	DataPoints             []jsonHistogramPoint `json:"dataPoints"`
}

type jsonHistogramPoint struct {
	Attributes        []jsonKeyValue `json:"attributes"`
	BucketCounts      []uint64       `json:"bucketCounts"`
	ExplicitBounds    []jsonFloat    `json:"explicitBounds"`
	Count             uint64         `json:"count"`
	Sum               jsonFloat      `json:"sum"`
	Min               *jsonFloat     `json:"min,omitempty"`
	Max               *jsonFloat     `json:"max,omitempty"`
	StartTimeUnixNano string         `json:"startTimeUnixNano"`
	TimeUnixNano      string         `json:"timeUnixNano"`
}

type jsonExponentialHistogram struct {
	AggregationTemporality int                    `json:"aggregationTemporality"`
	DataPoints             []jsonExponentialPoint `json:"dataPoints"`
}

type jsonExponentialPoint struct {
	Attributes        []jsonKeyValue         `json:"attributes"`
	Count             uint64                 `json:"count"`
	Min               *jsonFloat             `json:"min,omitempty"`
	Max               *jsonFloat             `json:"max,omitempty"`
	Sum               jsonFloat              `json:"sum"`
	Positive          jsonExponentialBuckets `json:"positive"`
	Negative          jsonExponentialBuckets `json:"negative"`
	Scale             int32                  `json:"scale"`
	ZeroCount         uint64                 `json:"zeroCount"`
	StartTimeUnixNano string                 `json:"startTimeUnixNano"`
	TimeUnixNano      string                 `json:"timeUnixNano"`
}

type jsonExponentialBuckets struct {
	Offset       int32    `json:"offset"`
	BucketCounts []uint64 `json:"bucketCounts"`
}

func otlpTemporality(t metricdata.Temporality) int {
	if t == metricdata.DeltaTemporality {
		return otlpTemporalityDelta
	}
	return otlpTemporalityCumulative
}

// MarshalMetrics encodes rm as an ExportMetricsServiceRequest
// (otlp-transformer createExportMetricsServiceRequest).
func MarshalMetrics(rm *metricdata.ResourceMetrics) ([]byte, error) {
	res := toJSONResource(rm.Resource)
	out := jsonResourceMetrics{Resource: res, SchemaURL: res.SchemaURL, ScopeMetrics: make([]jsonScopeMetrics, 0, len(rm.ScopeMetrics))}
	for _, sm := range rm.ScopeMetrics {
		js := jsonScopeMetrics{Scope: toJSONScope(sm.Scope), Metrics: make([]jsonMetric, 0, len(sm.Metrics)), SchemaURL: sm.Scope.SchemaURL}
		for _, m := range sm.Metrics {
			if jm, ok := toJSONMetric(m); ok {
				js.Metrics = append(js.Metrics, jm)
			}
		}
		out.ScopeMetrics = append(out.ScopeMetrics, js)
	}
	return marshalJSON(jsonMetricsRequest{ResourceMetrics: []jsonResourceMetrics{out}})
}

func toJSONMetric(m metricdata.Metrics) (jsonMetric, bool) {
	out := jsonMetric{Name: m.Name, Description: m.Description, Unit: m.Unit}
	switch d := m.Data.(type) {
	case metricdata.Gauge[int64]:
		out.Gauge = &jsonGauge{DataPoints: intPoints(d.DataPoints)}
	case metricdata.Gauge[float64]:
		out.Gauge = &jsonGauge{DataPoints: floatPoints(d.DataPoints)}
	case metricdata.Sum[int64]:
		out.Sum = &jsonSum{AggregationTemporality: otlpTemporality(d.Temporality), IsMonotonic: d.IsMonotonic, DataPoints: intPoints(d.DataPoints)}
	case metricdata.Sum[float64]:
		out.Sum = &jsonSum{AggregationTemporality: otlpTemporality(d.Temporality), IsMonotonic: d.IsMonotonic, DataPoints: floatPoints(d.DataPoints)}
	case metricdata.Histogram[int64]:
		out.Histogram = &jsonHistogram{AggregationTemporality: otlpTemporality(d.Temporality), DataPoints: histogramPoints(d.DataPoints)}
	case metricdata.Histogram[float64]:
		out.Histogram = &jsonHistogram{AggregationTemporality: otlpTemporality(d.Temporality), DataPoints: histogramPoints(d.DataPoints)}
	case metricdata.ExponentialHistogram[int64]:
		out.ExponentialHistogram = &jsonExponentialHistogram{AggregationTemporality: otlpTemporality(d.Temporality), DataPoints: exponentialPoints(d.DataPoints)}
	case metricdata.ExponentialHistogram[float64]:
		out.ExponentialHistogram = &jsonExponentialHistogram{AggregationTemporality: otlpTemporality(d.Temporality), DataPoints: exponentialPoints(d.DataPoints)}
	default:
		// Summaries have no Node counterpart and no Go SDK instrument produces them.
		return out, false
	}
	return out, true
}

func intPoints(points []metricdata.DataPoint[int64]) []jsonNumberPoint {
	out := make([]jsonNumberPoint, len(points))
	for i, p := range points {
		v := p.Value
		out[i] = jsonNumberPoint{Attributes: toJSONAttributes(p.Attributes.ToSlice()), StartTimeUnixNano: nanos(p.StartTime), TimeUnixNano: nanos(p.Time), AsInt: &v}
	}
	return out
}

func floatPoints(points []metricdata.DataPoint[float64]) []jsonNumberPoint {
	out := make([]jsonNumberPoint, len(points))
	for i, p := range points {
		out[i] = jsonNumberPoint{Attributes: toJSONAttributes(p.Attributes.ToSlice()), StartTimeUnixNano: nanos(p.StartTime), TimeUnixNano: nanos(p.Time), AsDouble: floatPtr(p.Value)}
	}
	return out
}

func extremaPtr[N int64 | float64](e metricdata.Extrema[N]) *jsonFloat {
	if v, ok := e.Value(); ok {
		return floatPtr(float64(v))
	}
	return nil
}

func histogramPoints[N int64 | float64](points []metricdata.HistogramDataPoint[N]) []jsonHistogramPoint {
	out := make([]jsonHistogramPoint, len(points))
	for i, p := range points {
		bounds := make([]jsonFloat, len(p.Bounds))
		for j, b := range p.Bounds {
			bounds[j] = jsonFloat(b)
		}
		counts := p.BucketCounts
		if counts == nil {
			counts = []uint64{}
		}
		out[i] = jsonHistogramPoint{
			Attributes:        toJSONAttributes(p.Attributes.ToSlice()),
			BucketCounts:      counts,
			ExplicitBounds:    bounds,
			Count:             p.Count,
			Sum:               jsonFloat(float64(p.Sum)),
			Min:               extremaPtr(p.Min),
			Max:               extremaPtr(p.Max),
			StartTimeUnixNano: nanos(p.StartTime),
			TimeUnixNano:      nanos(p.Time),
		}
	}
	return out
}

func exponentialPoints[N int64 | float64](points []metricdata.ExponentialHistogramDataPoint[N]) []jsonExponentialPoint {
	out := make([]jsonExponentialPoint, len(points))
	for i, p := range points {
		pos, neg := p.PositiveBucket.Counts, p.NegativeBucket.Counts
		if pos == nil {
			pos = []uint64{}
		}
		if neg == nil {
			neg = []uint64{}
		}
		out[i] = jsonExponentialPoint{
			Attributes:        toJSONAttributes(p.Attributes.ToSlice()),
			Count:             p.Count,
			Min:               extremaPtr(p.Min),
			Max:               extremaPtr(p.Max),
			Sum:               jsonFloat(float64(p.Sum)),
			Positive:          jsonExponentialBuckets{Offset: p.PositiveBucket.Offset, BucketCounts: pos},
			Negative:          jsonExponentialBuckets{Offset: p.NegativeBucket.Offset, BucketCounts: neg},
			Scale:             p.Scale,
			ZeroCount:         p.ZeroCount,
			StartTimeUnixNano: nanos(p.StartTime),
			TimeUnixNano:      nanos(p.Time),
		}
	}
	return out
}
