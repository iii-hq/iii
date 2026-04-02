import { beforeEach, describe, expect, it, vi } from 'vitest'
import { WorkerMetricsCollector } from '../src/worker-metrics'
import { registerWorkerGauges, stopWorkerGauges } from '../src/otel-worker-gauges'

type FakeGauge = { name: string }

describe('registerWorkerGauges', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    stopWorkerGauges()
  })

  it('registers gauges once, records metrics, and unregisters the callback on stop', () => {
    const gauges: FakeGauge[] = []
    let batchCallback: ((result: { observe: (...args: unknown[]) => void }) => void) | undefined

    vi.spyOn(WorkerMetricsCollector.prototype, 'collect').mockReturnValue({
      memory_heap_used: 10,
      memory_heap_total: 20,
      memory_rss: 30,
      memory_external: 40,
      cpu_percent: 50,
      cpu_user_micros: 60,
      cpu_system_micros: 70,
      event_loop_lag_ms: 80,
      uptime_seconds: 90,
      timestamp_ms: 123,
      runtime: 'node',
    })
    const stopMonitoringSpy = vi
      .spyOn(WorkerMetricsCollector.prototype, 'stopMonitoring')
      .mockImplementation(() => {})

    const meter = {
      createObservableGauge: vi.fn((name: string) => {
        const gauge = { name }
        gauges.push(gauge)
        return gauge
      }),
      addBatchObservableCallback: vi.fn((callback: typeof batchCallback) => {
        batchCallback = callback
      }),
      removeBatchObservableCallback: vi.fn(),
    }
    const batchResult = {
      observe: vi.fn(),
    }

    registerWorkerGauges(meter as never, {
      workerId: 'worker-123',
      workerName: 'coverage-worker',
    })
    registerWorkerGauges(meter as never, {
      workerId: 'worker-ignored',
    })

    expect(meter.createObservableGauge).toHaveBeenCalledTimes(9)
    expect(batchCallback).toBeTypeOf('function')

    batchCallback?.(batchResult)

    expect(batchResult.observe).toHaveBeenCalledTimes(9)
    expect(batchResult.observe).toHaveBeenCalledWith(
      gauges[0],
      10,
      expect.objectContaining({
        'worker.id': 'worker-123',
        'worker.name': 'coverage-worker',
      }),
    )

    stopWorkerGauges()

    expect(meter.removeBatchObservableCallback).toHaveBeenCalledOnce()
    expect(stopMonitoringSpy).toHaveBeenCalledOnce()
  })

  it('skips undefined metrics and handles stop without prior registration', () => {
    const gauges: FakeGauge[] = []
    let batchCallback: ((result: { observe: (...args: unknown[]) => void }) => void) | undefined

    vi.spyOn(WorkerMetricsCollector.prototype, 'collect').mockReturnValue({
      memory_heap_used: undefined,
      memory_heap_total: 20,
      memory_rss: undefined,
      memory_external: 40,
      cpu_percent: undefined,
      cpu_user_micros: 60,
      cpu_system_micros: undefined,
      event_loop_lag_ms: 80,
      uptime_seconds: undefined,
      timestamp_ms: 123,
      runtime: 'node',
    })

    const meter = {
      createObservableGauge: vi.fn((name: string) => {
        const gauge = { name }
        gauges.push(gauge)
        return gauge
      }),
      addBatchObservableCallback: vi.fn((callback: typeof batchCallback) => {
        batchCallback = callback
      }),
      removeBatchObservableCallback: vi.fn(),
    }
    const batchResult = {
      observe: vi.fn(),
    }

    stopWorkerGauges()
    registerWorkerGauges(meter as never, { workerId: 'worker-456' })

    batchCallback?.(batchResult)

    expect(batchResult.observe).toHaveBeenCalledTimes(4)
    expect(batchResult.observe).toHaveBeenCalledWith(gauges[1], 20, { 'worker.id': 'worker-456' })
  })
})
