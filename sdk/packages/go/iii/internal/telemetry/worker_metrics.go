package telemetry

import (
	"math"
	"runtime"
	"runtime/metrics"
	"sync"
	"time"
)

// schedLatencyMetric is the Go runtime's goroutine scheduling latency histogram: the time
// runnable goroutines wait before running, the closest Go analog of Node's event loop
// delay (monitorEventLoopDelay).
const schedLatencyMetric = "/sched/latencies:seconds"

// WorkerMetrics is one resource snapshot (Node WorkerMetrics). Fields the platform cannot
// measure are nil, as Node leaves them undefined.
type WorkerMetrics struct {
	MemoryHeapUsed  *float64 `json:"memory_heap_used,omitempty"`
	MemoryHeapTotal *float64 `json:"memory_heap_total,omitempty"`
	MemoryRSS       *float64 `json:"memory_rss,omitempty"`
	MemoryExternal  *float64 `json:"memory_external,omitempty"`
	CPUUserMicros   *float64 `json:"cpu_user_micros,omitempty"`
	CPUSystemMicros *float64 `json:"cpu_system_micros,omitempty"`
	CPUPercent      *float64 `json:"cpu_percent,omitempty"`
	EventLoopLagMs  *float64 `json:"event_loop_lag_ms,omitempty"`
	UptimeSeconds   *float64 `json:"uptime_seconds,omitempty"`
	TimestampMs     int64    `json:"timestamp_ms"`
	Runtime         string   `json:"runtime"`
}

// WorkerMetricsCollector collects WorkerMetrics (Node WorkerMetricsCollector): CPU usage
// since the previous collection, Go heap and resident memory, scheduling latency since
// the previous collection, and uptime.
type WorkerMetricsCollector struct {
	mu          sync.Mutex
	start       time.Time
	lastCPU     cpuTimes
	lastCPUOK   bool
	lastWall    time.Time
	lastSched   []uint64
	schedSample []metrics.Sample
}

// cpuTimes is cumulative process CPU time in microseconds.
type cpuTimes struct{ user, system float64 }

// NewWorkerMetricsCollector starts a collector; the first Collect reports CPU and
// scheduling latency since this call.
func NewWorkerMetricsCollector() *WorkerMetricsCollector {
	c := &WorkerMetricsCollector{
		start:       time.Now(),
		lastWall:    time.Now(),
		schedSample: []metrics.Sample{{Name: schedLatencyMetric}},
	}
	c.lastCPU, c.lastCPUOK = processCPUTimes()
	c.lastSched, _ = c.readSched()
	return c
}

// StopMonitoring releases the collector (Node stopMonitoring). The Go collector holds no
// background resources, so this only resets the latency baseline.
func (c *WorkerMetricsCollector) StopMonitoring() {
	c.mu.Lock()
	c.lastSched = nil
	c.mu.Unlock()
}

// Collect returns the current snapshot and resets the per-interval baselines.
func (c *WorkerMetricsCollector) Collect() WorkerMetrics {
	c.mu.Lock()
	defer c.mu.Unlock()

	now := time.Now()
	var ms runtime.MemStats
	runtime.ReadMemStats(&ms)
	out := WorkerMetrics{
		MemoryHeapUsed:  ptrFloat(float64(ms.HeapAlloc)),
		MemoryHeapTotal: ptrFloat(float64(ms.HeapSys)),
		UptimeSeconds:   ptrFloat(math.Floor(now.Sub(c.start).Seconds())),
		TimestampMs:     now.UnixMilli(),
		Runtime:         "go",
	}
	if rss, ok := residentSetSize(); ok {
		out.MemoryRSS = ptrFloat(rss)
	}

	if cpu, ok := processCPUTimes(); ok {
		out.CPUUserMicros = ptrFloat(cpu.user)
		out.CPUSystemMicros = ptrFloat(cpu.system)
		percent := 0.0
		if c.lastCPUOK {
			wallMicros := float64(now.Sub(c.lastWall).Microseconds())
			if wallMicros > 0 {
				percent = ((cpu.user - c.lastCPU.user) + (cpu.system - c.lastCPU.system)) / wallMicros * 100
			}
		}
		out.CPUPercent = ptrFloat(math.Min(math.Max(percent, 0), 100))
		c.lastCPU, c.lastCPUOK = cpu, true
	}
	c.lastWall = now

	if counts, buckets := c.readSched(); counts != nil {
		out.EventLoopLagMs = ptrFloat(meanLatencyMs(c.lastSched, counts, buckets))
		c.lastSched = counts
	}
	return out
}

// readSched returns a copy of the scheduling latency histogram counts and its buckets.
func (c *WorkerMetricsCollector) readSched() ([]uint64, []float64) {
	metrics.Read(c.schedSample)
	if c.schedSample[0].Value.Kind() != metrics.KindFloat64Histogram {
		return nil, nil
	}
	h := c.schedSample[0].Value.Float64Histogram()
	return append([]uint64(nil), h.Counts...), h.Buckets
}

// meanLatencyMs is the mean of the samples recorded between prev and cur, in
// milliseconds, using bucket midpoints (the finite edge for the open-ended buckets).
func meanLatencyMs(prev, cur []uint64, buckets []float64) float64 {
	var total, weighted float64
	for i, n := range cur {
		if i+1 >= len(buckets) {
			break
		}
		if prev != nil && i < len(prev) {
			if n < prev[i] {
				continue
			}
			n -= prev[i]
		}
		if n == 0 {
			continue
		}
		lo, hi := buckets[i], buckets[i+1]
		var mid float64
		switch {
		case math.IsInf(lo, -1):
			mid = hi
		case math.IsInf(hi, 1):
			mid = lo
		default:
			mid = (lo + hi) / 2
		}
		weighted += mid * float64(n)
		total += float64(n)
	}
	if total == 0 {
		return 0
	}
	return weighted / total * 1000
}

func ptrFloat(v float64) *float64 { return &v }
