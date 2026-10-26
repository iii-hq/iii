package telemetry

import (
	"context"
	"math"
	"runtime"
	"testing"

	"go.opentelemetry.io/otel/attribute"
	sdkmetric "go.opentelemetry.io/otel/sdk/metric"
	"go.opentelemetry.io/otel/sdk/metric/metricdata"
)

func TestWorkerMetricsCollector(t *testing.T) {
	c := NewWorkerMetricsCollector()
	m := c.Collect()
	if m.Runtime != "go" || m.TimestampMs <= 0 {
		t.Errorf("runtime %q timestamp %d", m.Runtime, m.TimestampMs)
	}
	if m.MemoryHeapUsed == nil || *m.MemoryHeapUsed <= 0 || m.MemoryHeapTotal == nil || *m.MemoryHeapTotal < *m.MemoryHeapUsed {
		t.Errorf("heap %v %v", m.MemoryHeapUsed, m.MemoryHeapTotal)
	}
	if m.UptimeSeconds == nil || *m.UptimeSeconds < 0 || m.EventLoopLagMs == nil || *m.EventLoopLagMs < 0 {
		t.Errorf("uptime %v lag %v", m.UptimeSeconds, m.EventLoopLagMs)
	}
	if m.MemoryExternal != nil {
		t.Error("Go has no external memory to report")
	}
	if runtime.GOOS == "linux" {
		if m.MemoryRSS == nil || *m.MemoryRSS <= 0 {
			t.Errorf("rss %v", m.MemoryRSS)
		}
		if m.CPUUserMicros == nil || m.CPUSystemMicros == nil || m.CPUPercent == nil || *m.CPUPercent < 0 || *m.CPUPercent > 100 {
			t.Errorf("cpu %v %v %v", m.CPUUserMicros, m.CPUSystemMicros, m.CPUPercent)
		}
	}
	c.StopMonitoring()
}

func TestMeanLatency(t *testing.T) {
	buckets := []float64{math.Inf(-1), 0.001, 0.003, math.Inf(1)} // -Inf, 1ms, 3ms, +Inf
	if got := meanLatencyMs(nil, []uint64{0, 2, 0}, buckets); got != 2 {
		t.Errorf("mean = %v, want 2 (midpoint of 1ms..3ms)", got)
	}
	if got := meanLatencyMs([]uint64{0, 2, 0}, []uint64{1, 2, 1}, buckets); got != 2 {
		t.Errorf("delta mean = %v, want 2 (1ms and 3ms edges)", got)
	}
	if got := meanLatencyMs([]uint64{1, 2, 1}, []uint64{1, 2, 1}, buckets); got != 0 {
		t.Errorf("no samples -> %v", got)
	}
}

// The gauges of Node otel-worker-gauges.ts: names, units, worker attributes, and gone
// after Stop.
func TestRegisterWorkerGauges(t *testing.T) {
	reader := sdkmetric.NewManualReader()
	mp := sdkmetric.NewMeterProvider(sdkmetric.WithReader(reader))
	g, err := RegisterWorkerGauges(mp.Meter("svc"), WorkerGaugesOptions{WorkerID: "w-1", WorkerName: "orders"})
	if err != nil {
		t.Fatal(err)
	}
	var rm metricdata.ResourceMetrics
	if err := reader.Collect(context.Background(), &rm); err != nil {
		t.Fatal(err)
	}
	got := map[string]metricdata.Metrics{}
	for _, m := range rm.ScopeMetrics[0].Metrics {
		got[m.Name] = m
	}
	for _, spec := range workerGaugeSpecs {
		m, ok := got[spec.name]
		if !ok {
			if spec.name == "iii.worker.memory.external" || (runtime.GOOS != "linux" && spec.name == "iii.worker.memory.rss") {
				continue
			}
			t.Errorf("gauge %s not reported", spec.name)
			continue
		}
		if m.Unit != spec.unit || m.Description != spec.description {
			t.Errorf("%s: unit %q description %q", spec.name, m.Unit, m.Description)
		}
		point := m.Data.(metricdata.Gauge[float64]).DataPoints[0]
		want := attribute.NewSet(attribute.String("worker.id", "w-1"), attribute.String("worker.name", "orders"))
		if !point.Attributes.Equals(&want) {
			t.Errorf("%s attributes %v", spec.name, point.Attributes.ToSlice())
		}
	}
	if err := g.Stop(); err != nil {
		t.Fatal(err)
	}
	_ = g.Stop()
	rm = metricdata.ResourceMetrics{}
	_ = reader.Collect(context.Background(), &rm)
	for _, sm := range rm.ScopeMetrics {
		for _, m := range sm.Metrics {
			if gauge, ok := m.Data.(metricdata.Gauge[float64]); ok && len(gauge.DataPoints) > 0 {
				t.Errorf("%s still observed after Stop", m.Name)
			}
		}
	}
}
