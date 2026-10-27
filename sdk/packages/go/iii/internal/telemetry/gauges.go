package telemetry

import (
	"context"
	"sync"

	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
)

// WorkerGaugesOptions identifies the worker the gauges report for (Node
// WorkerGaugesOptions).
type WorkerGaugesOptions struct {
	WorkerID   string
	WorkerName string
}

// WorkerGauges is a registration of the worker resource gauges; Stop removes it.
type WorkerGauges struct {
	reg       metric.Registration
	collector *WorkerMetricsCollector
	once      sync.Once
}

type gaugeSpec struct {
	name, description, unit string
	value                   func(WorkerMetrics) *float64
}

// workerGaugeSpecs are the observable gauges of Node otel-worker-gauges.ts, with the same
// names, descriptions and units.
var workerGaugeSpecs = []gaugeSpec{
	{"iii.worker.memory.heap_used", "Worker heap memory used in bytes", "bytes", func(m WorkerMetrics) *float64 { return m.MemoryHeapUsed }},
	{"iii.worker.memory.heap_total", "Worker total heap memory in bytes", "bytes", func(m WorkerMetrics) *float64 { return m.MemoryHeapTotal }},
	{"iii.worker.memory.rss", "Worker resident set size in bytes", "bytes", func(m WorkerMetrics) *float64 { return m.MemoryRSS }},
	{"iii.worker.memory.external", "Worker external memory in bytes", "bytes", func(m WorkerMetrics) *float64 { return m.MemoryExternal }},
	{"iii.worker.cpu.percent", "Worker CPU usage percentage", "%", func(m WorkerMetrics) *float64 { return m.CPUPercent }},
	{"iii.worker.cpu.user_micros", "Worker CPU user time in microseconds", "us", func(m WorkerMetrics) *float64 { return m.CPUUserMicros }},
	{"iii.worker.cpu.system_micros", "Worker CPU system time in microseconds", "us", func(m WorkerMetrics) *float64 { return m.CPUSystemMicros }},
	{"iii.worker.event_loop.lag_ms", "Worker event loop lag in milliseconds", "ms", func(m WorkerMetrics) *float64 { return m.EventLoopLagMs }},
	{"iii.worker.uptime_seconds", "Worker uptime in seconds", "s", func(m WorkerMetrics) *float64 { return m.UptimeSeconds }},
}

// RegisterWorkerGauges registers the worker resource gauges on meter (Node
// registerWorkerGauges): one batch callback collects a snapshot per collection cycle and
// observes every measured value with the worker.id and worker.name attributes.
func RegisterWorkerGauges(meter metric.Meter, opts WorkerGaugesOptions) (*WorkerGauges, error) {
	attrs := []attribute.KeyValue{attribute.String("worker.id", opts.WorkerID)}
	if opts.WorkerName != "" {
		attrs = append(attrs, attribute.String("worker.name", opts.WorkerName))
	}
	set := attribute.NewSet(attrs...)

	gauges := make([]metric.Float64ObservableGauge, len(workerGaugeSpecs))
	instruments := make([]metric.Observable, len(workerGaugeSpecs))
	for i, spec := range workerGaugeSpecs {
		g, err := meter.Float64ObservableGauge(spec.name, metric.WithDescription(spec.description), metric.WithUnit(spec.unit))
		if err != nil {
			return nil, err
		}
		gauges[i] = g
		instruments[i] = g
	}

	collector := NewWorkerMetricsCollector()
	reg, err := meter.RegisterCallback(func(_ context.Context, o metric.Observer) error {
		m := collector.Collect()
		for i, spec := range workerGaugeSpecs {
			if v := spec.value(m); v != nil {
				o.ObserveFloat64(gauges[i], *v, metric.WithAttributeSet(set))
			}
		}
		return nil
	}, instruments...)
	if err != nil {
		return nil, err
	}
	return &WorkerGauges{reg: reg, collector: collector}, nil
}

// Stop unregisters the gauges (Node stopWorkerGauges). It is idempotent.
func (g *WorkerGauges) Stop() error {
	var err error
	g.once.Do(func() {
		err = g.reg.Unregister()
		g.collector.StopMonitoring()
	})
	return err
}
