package telemetry

import (
	"context"
	"fmt"
	"sync"

	sdklog "go.opentelemetry.io/otel/sdk/log"
	sdkmetric "go.opentelemetry.io/otel/sdk/metric"
	"go.opentelemetry.io/otel/sdk/metric/metricdata"
	sdktrace "go.opentelemetry.io/otel/sdk/trace"
)

// MaxPendingExports bounds each exporter's queue while the connection is not open
// (MAX_PENDING_EXPORTS in the Node span, log and metric exporters).
const MaxPendingExports = 100

// frameExporter is the shared core of the three Node exporters (EngineSpanExporter,
// EngineLogExporter, EngineMetricsExporter): send when connected, queue up to
// MaxPendingExports while connecting, drop with an error when the connection failed or
// is shutting down, flush the queue on (re)connection.
type frameExporter struct {
	conn   *Connection
	prefix string
	signal string

	mu      sync.Mutex
	pending [][]byte
}

func newFrameExporter(conn *Connection, prefix, signal string) *frameExporter {
	e := &frameExporter{conn: conn, prefix: prefix, signal: signal}
	conn.OnConnected(e.flushPending)
	conn.OnFailed(e.dropPending)
	return e
}

func (e *frameExporter) export(payload []byte) error {
	if state := e.conn.State(); state != StateConnected {
		if state == StateFailed || e.conn.IsShuttingDown() {
			reason := "shut down"
			if state == StateFailed {
				reason = "failed"
			}
			return fmt.Errorf("telemetry: connection %s: dropping %s", reason, e.signal)
		}
		e.mu.Lock()
		if len(e.pending) >= MaxPendingExports {
			e.pending = append(e.pending[:0:0], e.pending[1:]...)
			logf("[OTel] %s export queue full, dropped oldest entry", e.signal)
		}
		e.pending = append(e.pending, payload)
		e.mu.Unlock()
		// The socket may have opened (and run the flush) between the state check and the
		// enqueue; flush again so the entry does not wait for the next reconnection.
		if e.conn.State() == StateConnected {
			e.flushPending()
		}
		return nil
	}
	return e.conn.Send(e.prefix, payload)
}

func (e *frameExporter) flushPending() {
	e.mu.Lock()
	pending := e.pending
	e.pending = nil
	e.mu.Unlock()
	for _, p := range pending {
		if err := e.conn.Send(e.prefix, p); err != nil {
			logf("[OTel] Failed to send %s: %v", e.signal, err)
		}
	}
}

func (e *frameExporter) dropPending() {
	e.mu.Lock()
	e.pending = nil
	e.mu.Unlock()
}

// SpanExporter sends spans as OTLP frames (Node EngineSpanExporter).
type SpanExporter struct{ core *frameExporter }

// NewSpanExporter returns a span exporter writing to conn.
func NewSpanExporter(conn *Connection) *SpanExporter {
	return &SpanExporter{core: newFrameExporter(conn, PrefixTraces, "spans")}
}

// ExportSpans implements sdktrace.SpanExporter.
func (e *SpanExporter) ExportSpans(_ context.Context, spans []sdktrace.ReadOnlySpan) error {
	if len(spans) == 0 {
		return nil
	}
	payload, err := MarshalTraces(spans)
	if err != nil {
		return err
	}
	return e.core.export(payload)
}

// Shutdown implements sdktrace.SpanExporter: queued exports are dropped.
func (e *SpanExporter) Shutdown(context.Context) error {
	e.core.dropPending()
	return nil
}

// LogExporter sends log records as LOGS frames (Node EngineLogExporter).
type LogExporter struct{ core *frameExporter }

// NewLogExporter returns a log exporter writing to conn.
func NewLogExporter(conn *Connection) *LogExporter {
	return &LogExporter{core: newFrameExporter(conn, PrefixLogs, "logs")}
}

// Export implements sdklog.Exporter.
func (e *LogExporter) Export(_ context.Context, records []sdklog.Record) error {
	if len(records) == 0 {
		return nil
	}
	payload, err := MarshalLogs(records)
	if err != nil {
		return err
	}
	return e.core.export(payload)
}

// ForceFlush implements sdklog.Exporter; frames are sent as they are exported.
func (e *LogExporter) ForceFlush(context.Context) error { return nil }

// Shutdown implements sdklog.Exporter: queued exports are dropped.
func (e *LogExporter) Shutdown(context.Context) error {
	e.core.dropPending()
	return nil
}

// MetricExporter sends metrics as MTRC frames (Node EngineMetricsExporter), with the
// SDK's default (cumulative) temporality and aggregations, as the Node
// PeriodicExportingMetricReader uses.
type MetricExporter struct{ core *frameExporter }

// NewMetricExporter returns a metric exporter writing to conn.
func NewMetricExporter(conn *Connection) *MetricExporter {
	return &MetricExporter{core: newFrameExporter(conn, PrefixMetrics, "metrics")}
}

// Temporality implements sdkmetric.Exporter.
func (e *MetricExporter) Temporality(k sdkmetric.InstrumentKind) metricdata.Temporality {
	return sdkmetric.DefaultTemporalitySelector(k)
}

// Aggregation implements sdkmetric.Exporter.
func (e *MetricExporter) Aggregation(k sdkmetric.InstrumentKind) sdkmetric.Aggregation {
	return sdkmetric.DefaultAggregationSelector(k)
}

// Export implements sdkmetric.Exporter. A collection with no scope metrics is not sent,
// as the Node PeriodicExportingMetricReader skips it.
func (e *MetricExporter) Export(_ context.Context, rm *metricdata.ResourceMetrics) error {
	if rm == nil || len(rm.ScopeMetrics) == 0 {
		return nil
	}
	payload, err := MarshalMetrics(rm)
	if err != nil {
		return err
	}
	return e.core.export(payload)
}

// ForceFlush implements sdkmetric.Exporter; frames are sent as they are exported.
func (e *MetricExporter) ForceFlush(context.Context) error { return nil }

// Shutdown implements sdkmetric.Exporter: queued exports are dropped.
func (e *MetricExporter) Shutdown(context.Context) error {
	e.core.dropPending()
	return nil
}

var (
	_ sdktrace.SpanExporter = (*SpanExporter)(nil)
	_ sdklog.Exporter       = (*LogExporter)(nil)
	_ sdkmetric.Exporter    = (*MetricExporter)(nil)
)
