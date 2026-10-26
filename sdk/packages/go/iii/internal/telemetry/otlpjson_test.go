package telemetry

import (
	"bytes"
	"context"
	"math"
	"os"
	"sync"
	"testing"
	"time"

	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/codes"
	otellog "go.opentelemetry.io/otel/log"
	"go.opentelemetry.io/otel/sdk/instrumentation"
	sdklog "go.opentelemetry.io/otel/sdk/log"
	"go.opentelemetry.io/otel/sdk/metric/metricdata"
	"go.opentelemetry.io/otel/sdk/resource"
	sdktrace "go.opentelemetry.io/otel/sdk/trace"
	"go.opentelemetry.io/otel/sdk/trace/tracetest"
	"go.opentelemetry.io/otel/trace"
)

// These tests encode the same spans, log records and metrics that
// testdata/node/generate.cjs feeds to the Node SDK's serializers
// (@opentelemetry/otlp-transformer 0.222.0 JsonTraceSerializer, JsonLogsSerializer,
// JsonMetricsSerializer) and require the Go output to match the Node output byte for
// byte.

var (
	fixtureTraceID, _ = trace.TraceIDFromHex("4bf92f3577b34da6a3ce929d0e0e4736")
	fixtureSpanID, _  = trace.SpanIDFromHex("00f067aa0ba902b7")
)

func fixtureResource(t *testing.T) *resource.Resource {
	t.Helper()
	for _, k := range []string{"OTEL_RESOURCE_ATTRIBUTES", "OTEL_SERVICE_NAME"} {
		t.Setenv(k, "")
	}
	return resource.NewSchemaless(
		attribute.String("service.name", "svc"),
		attribute.String("service.version", "1.2.3"),
		attribute.String("service.instance.id", "inst-1"),
	)
}

func nodeFixture(t *testing.T, name string) []byte {
	t.Helper()
	b, err := os.ReadFile("testdata/node/" + name)
	if err != nil {
		t.Fatal(err)
	}
	return bytes.TrimSuffix(b, []byte("\n"))
}

func assertSameJSON(t *testing.T, got, want []byte) {
	t.Helper()
	if !bytes.Equal(got, want) {
		t.Errorf("Go serializer output differs from the Node serializer\n got: %s\nwant: %s", got, want)
	}
}

func TestMarshalTracesMatchesNode(t *testing.T) {
	res := fixtureResource(t)
	scope := instrumentation.Scope{Name: "svc"}
	parentID, _ := trace.SpanIDFromHex("b7ad6b7169203331")
	clientID, _ := trace.SpanIDFromHex("2222222222222222")
	linkID, _ := trace.SpanIDFromHex("1111111111111111")
	stubs := tracetest.SpanStubs{
		{
			Name:        "execute orders::create",
			SpanContext: trace.NewSpanContext(trace.SpanContextConfig{TraceID: fixtureTraceID, SpanID: fixtureSpanID, TraceFlags: trace.FlagsSampled}),
			Parent:      trace.NewSpanContext(trace.SpanContextConfig{TraceID: fixtureTraceID, SpanID: parentID, TraceFlags: trace.FlagsSampled, Remote: true}),
			SpanKind:    trace.SpanKindInternal,
			StartTime:   time.Unix(1700000000, 123),
			EndTime:     time.Unix(1700000001, 456),
			Attributes: []attribute.KeyValue{
				attribute.String("tenant", "acme"),
				attribute.Int64("n", 42),
				attribute.Float64("ratio", 0.5),
				attribute.Bool("ok", true),
				attribute.StringSlice("tags", []string{"a", "b"}),
			},
			Events: []sdktrace.Event{{
				Name: "iii.invocation.input",
				Time: time.Unix(1700000000, 500),
				Attributes: []attribute.KeyValue{
					attribute.String("iii.payload.json", `{"a":1}`),
					attribute.Bool("iii.payload.truncated", false),
				},
			}},
			Status:               sdktrace.Status{Code: codes.Error, Description: "boom"},
			Resource:             res,
			InstrumentationScope: scope,
		},
		{
			Name:        "GET /api",
			SpanContext: trace.NewSpanContext(trace.SpanContextConfig{TraceID: fixtureTraceID, SpanID: clientID, TraceFlags: trace.FlagsSampled}),
			SpanKind:    trace.SpanKindClient,
			StartTime:   time.Unix(1700000002, 0),
			EndTime:     time.Unix(1700000002, 999999999),
			Attributes: []attribute.KeyValue{
				attribute.String("http.request.method", "GET"),
				attribute.Int("server.port", 8080),
			},
			Links: []sdktrace.Link{{
				SpanContext: trace.NewSpanContext(trace.SpanContextConfig{TraceID: fixtureTraceID, SpanID: linkID, TraceFlags: trace.FlagsSampled, Remote: true}),
				Attributes:  []attribute.KeyValue{attribute.String("k", "v")},
			}},
			Status:               sdktrace.Status{Code: codes.Ok},
			Resource:             res,
			InstrumentationScope: scope,
		},
	}
	got, err := MarshalTraces(stubs.Snapshots())
	if err != nil {
		t.Fatal(err)
	}
	assertSameJSON(t, got, nodeFixture(t, "traces.json"))
}

type captureProcessor struct {
	mu      sync.Mutex
	records []sdklog.Record
}

func (p *captureProcessor) OnEmit(_ context.Context, r *sdklog.Record) error {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.records = append(p.records, r.Clone())
	return nil
}

// Enabled reports that every record is wanted (sdklog.Processor since otel/sdk/log v0.15).
func (p *captureProcessor) Enabled(context.Context, sdklog.EnabledParameters) bool { return true }
func (p *captureProcessor) Shutdown(context.Context) error                         { return nil }
func (p *captureProcessor) ForceFlush(context.Context) error                       { return nil }

func TestMarshalLogsMatchesNode(t *testing.T) {
	capture := &captureProcessor{}
	lp := sdklog.NewLoggerProvider(sdklog.WithResource(fixtureResource(t)), sdklog.WithProcessor(capture))
	logger := lp.Logger("svc")

	sc := trace.NewSpanContext(trace.SpanContextConfig{TraceID: fixtureTraceID, SpanID: fixtureSpanID, TraceFlags: trace.FlagsSampled})
	var first otellog.Record
	first.SetTimestamp(time.Unix(1700000000, 1000))
	first.SetObservedTimestamp(time.Unix(1700000000, 2000))
	first.SetSeverity(otellog.SeverityInfo)
	first.SetBody(attribute.StringValue("Order processed"))
	first.AddAttributes(
		attribute.String("trace_id", fixtureTraceID.String()),
		attribute.String("span_id", fixtureSpanID.String()),
		attribute.KeyValue{Key: "log.data", Value: ToLogValue(map[string]any{
			"orderId": "ord_123", "amount": 49.99, "count": 3, "tags": []string{"x"}, "nested": map[string]bool{"ok": true},
		})},
	)
	logger.Emit(trace.ContextWithSpanContext(context.Background(), sc), first)

	var second otellog.Record
	second.SetTimestamp(time.Unix(1700000003, 0))
	second.SetObservedTimestamp(time.Unix(1700000003, 5))
	second.SetSeverity(otellog.SeverityError)
	second.SetBody(attribute.StringValue("no span"))
	logger.Emit(context.Background(), second)

	got, err := MarshalLogs(capture.records)
	if err != nil {
		t.Fatal(err)
	}
	assertSameJSON(t, got, nodeFixture(t, "logs.json"))
}

func TestMarshalMetricsMatchesNode(t *testing.T) {
	start, end := time.Unix(1700000000, 0), time.Unix(1700000060, 0)
	rm := &metricdata.ResourceMetrics{
		Resource: fixtureResource(t),
		ScopeMetrics: []metricdata.ScopeMetrics{{
			Scope: instrumentation.Scope{Name: "svc"},
			Metrics: []metricdata.Metrics{
				{
					Name: "iii.worker.memory.heap_used", Description: "Worker heap memory used in bytes", Unit: "bytes",
					Data: metricdata.Gauge[float64]{DataPoints: []metricdata.DataPoint[float64]{{
						Attributes: attribute.NewSet(attribute.String("worker.name", "orders"), attribute.String("worker.id", "w-1")),
						StartTime:  start, Time: end, Value: 1048576.5,
					}}},
				},
				{
					Name: "requests", Unit: "1",
					Data: metricdata.Sum[int64]{Temporality: metricdata.CumulativeTemporality, IsMonotonic: true, DataPoints: []metricdata.DataPoint[int64]{{
						Attributes: attribute.NewSet(attribute.String("route", "/a")),
						StartTime:  start, Time: end, Value: 7,
					}}},
				},
				{
					Name: "latency", Description: "d", Unit: "ms",
					Data: metricdata.Histogram[float64]{Temporality: metricdata.DeltaTemporality, DataPoints: []metricdata.HistogramDataPoint[float64]{{
						StartTime: start, Time: end, Count: 4, Bounds: []float64{0, 5, 10}, BucketCounts: []uint64{1, 2, 0, 1},
						Min: metricdata.NewExtrema(0.5), Max: metricdata.NewExtrema(12.0), Sum: 20.5,
					}}},
				},
			},
		}},
	}
	got, err := MarshalMetrics(rm)
	if err != nil {
		t.Fatal(err)
	}
	assertSameJSON(t, got, nodeFixture(t, "metrics.json"))
}

// JSON.stringify writes NaN and the infinities as null; encoding/json would fail.
func TestMarshalNonFiniteFloatsAsNull(t *testing.T) {
	res := fixtureResource(t)
	stub := tracetest.SpanStub{
		Name:        "nan",
		SpanContext: trace.NewSpanContext(trace.SpanContextConfig{TraceID: fixtureTraceID, SpanID: fixtureSpanID}),
		Attributes:  []attribute.KeyValue{attribute.Float64("x", math.NaN()), attribute.Float64("y", math.Inf(1))},
		Resource:    res,
	}
	got, err := MarshalTraces(tracetest.SpanStubs{stub}.Snapshots())
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Contains(got, []byte(`{"key":"x","value":{"doubleValue":null}}`)) || !bytes.Contains(got, []byte(`{"key":"y","value":{"doubleValue":null}}`)) {
		t.Errorf("non-finite floats not encoded as null: %s", got)
	}
	// The zero time encodes as "0" (the engine rejects negative timestamps).
	if !bytes.Contains(got, []byte(`"startTimeUnixNano":"0"`)) {
		t.Errorf("zero start time not encoded as \"0\": %s", got)
	}
}
