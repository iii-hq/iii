import { performance } from 'node:perf_hooks'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { WorkerMetricsCollector } from '../src/worker-metrics'

type MutableCollectorInternals = {
  lastCpuUsage: NodeJS.CpuUsage
  lastCpuTime: number
  eventLoopHistogram: unknown
}

describe('WorkerMetricsCollector', () => {
  beforeEach(() => {
    vi.restoreAllMocks()
    vi.clearAllMocks()
  })

  it('collects memory and cpu metrics and caps cpu percent at 100', () => {
    const histogram = {
      mean: 8_000_000,
      reset: vi.fn(),
      disable: vi.fn(),
    }

    vi.spyOn(Date, 'now').mockReturnValue(10_000)
    vi.spyOn(process, 'cpuUsage').mockReturnValue({ user: 401_000, system: 101_000 })
    vi.spyOn(process, 'memoryUsage').mockReturnValue({
      rss: 1_024,
      heapTotal: 2_048,
      heapUsed: 1_536,
      external: 256,
      arrayBuffers: 128,
    })
    vi.spyOn(performance, 'now').mockReturnValue(1_500)

    const collector = new WorkerMetricsCollector({ eventLoopResolutionMs: 0 })
    const mutableCollector = collector as unknown as MutableCollectorInternals
    mutableCollector.lastCpuUsage = { user: 1_000, system: 500 }
    mutableCollector.lastCpuTime = 1_000
    mutableCollector.eventLoopHistogram = histogram
    const metrics = collector.collect()

    expect(histogram.reset).toHaveBeenCalledOnce()
    expect(metrics).toMatchObject({
      memory_rss: 1_024,
      memory_heap_total: 2_048,
      memory_heap_used: 1_536,
      memory_external: 256,
      cpu_user_micros: 401_000,
      cpu_system_micros: 101_000,
      cpu_percent: 100,
      event_loop_lag_ms: 8,
      uptime_seconds: 0,
      timestamp_ms: 10_000,
      runtime: 'node',
    })
  })

  it('stops monitoring and clears the histogram reference', () => {
    const histogram = {
      mean: 0,
      reset: vi.fn(),
      disable: vi.fn(),
    }

    vi.spyOn(Date, 'now').mockReturnValue(20_500)
    vi.spyOn(process, 'cpuUsage').mockReturnValue({ user: 1_000, system: 500 })
    vi.spyOn(process, 'memoryUsage').mockReturnValue({
      rss: 2_048,
      heapTotal: 4_096,
      heapUsed: 3_072,
      external: 512,
      arrayBuffers: 128,
    })
    vi.spyOn(performance, 'now').mockReturnValue(600)

    const collector = new WorkerMetricsCollector({ eventLoopResolutionMs: 5.8 })
    const mutableCollector = collector as unknown as MutableCollectorInternals
    mutableCollector.lastCpuUsage = { user: 500, system: 250 }
    mutableCollector.lastCpuTime = 100
    mutableCollector.eventLoopHistogram = histogram
    const metrics = collector.collect()
    collector.stopMonitoring()

    expect(metrics.cpu_percent).toBeCloseTo(0.15)
    expect(metrics.event_loop_lag_ms).toBe(0)
    expect(histogram.disable).toHaveBeenCalledOnce()
    expect(mutableCollector.eventLoopHistogram).toBeNull()
  })
})
