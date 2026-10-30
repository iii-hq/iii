package observability

import (
	"context"

	sdktrace "go.opentelemetry.io/otel/sdk/trace"

	"github.com/iii-hq/iii/sdk/packages/go/iii/internal/telemetry"
)

// WorkerMetrics is one snapshot of the worker's resource usage (Node WorkerMetrics). A
// field the platform cannot measure is nil.
type WorkerMetrics struct {
	// MemoryHeapUsed is the Go heap in use, in bytes (runtime.MemStats.HeapAlloc).
	MemoryHeapUsed *float64 `json:"memory_heap_used,omitempty"`
	// MemoryHeapTotal is the heap memory obtained from the OS, in bytes
	// (runtime.MemStats.HeapSys).
	MemoryHeapTotal *float64 `json:"memory_heap_total,omitempty"`
	// MemoryRSS is the resident set size in bytes (Linux).
	MemoryRSS *float64 `json:"memory_rss,omitempty"`
	// MemoryExternal is Node's memory held outside the JavaScript heap; Go has no
	// equivalent and leaves it nil.
	MemoryExternal *float64 `json:"memory_external,omitempty"`
	// CPUUserMicros is the cumulative user CPU time in microseconds (Unix).
	CPUUserMicros *float64 `json:"cpu_user_micros,omitempty"`
	// CPUSystemMicros is the cumulative system CPU time in microseconds (Unix).
	CPUSystemMicros *float64 `json:"cpu_system_micros,omitempty"`
	// CPUPercent is the CPU usage since the previous collection, capped at 100 (Unix).
	CPUPercent *float64 `json:"cpu_percent,omitempty"`
	// EventLoopLagMs is the mean goroutine scheduling latency since the previous
	// collection, in milliseconds: the Go counterpart of Node's event loop lag.
	EventLoopLagMs *float64 `json:"event_loop_lag_ms,omitempty"`
	// UptimeSeconds is the collector's age in whole seconds.
	UptimeSeconds *float64 `json:"uptime_seconds,omitempty"`
	// TimestampMs is the collection time in Unix milliseconds.
	TimestampMs int64 `json:"timestamp_ms"`
	// Runtime is "go".
	Runtime string `json:"runtime"`
}

// WorkerMetricsCollector collects [WorkerMetrics] (Node WorkerMetricsCollector).
type WorkerMetricsCollector struct {
	inner *telemetry.WorkerMetricsCollector
}

// NewWorkerMetricsCollector starts a collector. The first Collect reports CPU usage and
// scheduling latency since this call; each later one since the previous Collect.
func NewWorkerMetricsCollector() *WorkerMetricsCollector {
	return &WorkerMetricsCollector{inner: telemetry.NewWorkerMetricsCollector()}
}

// Collect returns the current snapshot.
func (c *WorkerMetricsCollector) Collect() WorkerMetrics {
	m := c.inner.Collect()
	return WorkerMetrics{
		MemoryHeapUsed:  m.MemoryHeapUsed,
		MemoryHeapTotal: m.MemoryHeapTotal,
		MemoryRSS:       m.MemoryRSS,
		MemoryExternal:  m.MemoryExternal,
		CPUUserMicros:   m.CPUUserMicros,
		CPUSystemMicros: m.CPUSystemMicros,
		CPUPercent:      m.CPUPercent,
		EventLoopLagMs:  m.EventLoopLagMs,
		UptimeSeconds:   m.UptimeSeconds,
		TimestampMs:     m.TimestampMs,
		Runtime:         m.Runtime,
	}
}

// StopMonitoring releases the collector (Node stopMonitoring). The Go collector holds no
// background resources; the call resets its scheduling latency baseline.
func (c *WorkerMetricsCollector) StopMonitoring() { c.inner.StopMonitoring() }

// WorkerGaugesOptions identifies the worker the gauges report for (Node
// WorkerGaugesOptions).
type WorkerGaugesOptions struct {
	// WorkerID is the stable identifier of the worker, reported as worker.id.
	WorkerID string
	// WorkerName is the human-readable name of the worker, reported as worker.name
	// when set.
	WorkerName string
}

// WorkerGauges is a registration of the worker resource gauges, returned by
// [RegisterWorkerGauges].
type WorkerGauges struct {
	inner *telemetry.WorkerGauges
}

// RegisterWorkerGauges registers the worker resource gauges on meter (Node
// registerWorkerGauges): iii.worker.memory.heap_used, iii.worker.memory.heap_total,
// iii.worker.memory.rss, iii.worker.memory.external, iii.worker.cpu.percent,
// iii.worker.cpu.user_micros, iii.worker.cpu.system_micros,
// iii.worker.event_loop.lag_ms and iii.worker.uptime_seconds, each observed once per
// collection with the worker.id and worker.name attributes (values the platform cannot
// measure are skipped). A client does this itself when iii.WithMetricsReporting is on;
// call it for a meter of your own.
func RegisterWorkerGauges(meter Meter, opts WorkerGaugesOptions) (*WorkerGauges, error) {
	g, err := telemetry.RegisterWorkerGauges(meter, telemetry.WorkerGaugesOptions{WorkerID: opts.WorkerID, WorkerName: opts.WorkerName})
	if err != nil {
		return nil, err
	}
	return &WorkerGauges{inner: g}, nil
}

// Stop unregisters the gauges (Node stopWorkerGauges). It is idempotent.
func (g *WorkerGauges) Stop() error { return g.inner.Stop() }

// BaggageSpanProcessor is an OpenTelemetry span processor that copies every baggage
// member of a span's parent context onto the span as a string attribute when it starts
// (Node BaggageSpanProcessor). The client's tracer provider already runs it; add it to
// a tracer provider of your own with sdktrace.WithSpanProcessor, before the exporting
// processor.
type BaggageSpanProcessor struct{}

// OnStart copies the baggage of parent onto s.
func (BaggageSpanProcessor) OnStart(parent context.Context, s sdktrace.ReadWriteSpan) {
	telemetry.BaggageSpanProcessor{}.OnStart(parent, s)
}

// OnEnd does nothing.
func (BaggageSpanProcessor) OnEnd(sdktrace.ReadOnlySpan) {}

// Shutdown does nothing.
func (BaggageSpanProcessor) Shutdown(context.Context) error { return nil }

// ForceFlush does nothing.
func (BaggageSpanProcessor) ForceFlush(context.Context) error { return nil }

var _ sdktrace.SpanProcessor = BaggageSpanProcessor{}
