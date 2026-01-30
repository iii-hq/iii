import type { Meter, BatchObservableResult } from '@opentelemetry/api'
import { WorkerMetricsCollector } from './worker-metrics'

export interface WorkerGaugesOptions {
  workerId: string
  workerName?: string
}

let registeredGauges = false
let metricsCollector: WorkerMetricsCollector | null = null

export function registerWorkerGauges(meter: Meter, options: WorkerGaugesOptions): void {
  if (registeredGauges) {
    return
  }

  const { workerId, workerName } = options
  const baseAttributes = {
    'worker.id': workerId,
    ...(workerName && { 'worker.name': workerName }),
  }

  metricsCollector = new WorkerMetricsCollector()

  const memoryHeapUsed = meter.createObservableGauge('iii.worker.memory.heap_used', {
    description: 'Worker heap memory used in bytes',
    unit: 'bytes',
  })

  const memoryHeapTotal = meter.createObservableGauge('iii.worker.memory.heap_total', {
    description: 'Worker total heap memory in bytes',
    unit: 'bytes',
  })

  const memoryRss = meter.createObservableGauge('iii.worker.memory.rss', {
    description: 'Worker resident set size in bytes',
    unit: 'bytes',
  })

  const memoryExternal = meter.createObservableGauge('iii.worker.memory.external', {
    description: 'Worker external memory in bytes',
    unit: 'bytes',
  })

  const cpuPercent = meter.createObservableGauge('iii.worker.cpu.percent', {
    description: 'Worker CPU usage percentage',
    unit: '%',
  })

  const cpuUserMicros = meter.createObservableGauge('iii.worker.cpu.user_micros', {
    description: 'Worker CPU user time in microseconds',
    unit: 'us',
  })

  const cpuSystemMicros = meter.createObservableGauge('iii.worker.cpu.system_micros', {
    description: 'Worker CPU system time in microseconds',
    unit: 'us',
  })

  const eventLoopLag = meter.createObservableGauge('iii.worker.event_loop.lag_ms', {
    description: 'Worker event loop lag in milliseconds',
    unit: 'ms',
  })

  const uptimeSeconds = meter.createObservableGauge('iii.worker.uptime_seconds', {
    description: 'Worker uptime in seconds',
    unit: 's',
  })

  const batchCallback = (observableResult: BatchObservableResult) => {
    if (!metricsCollector) return

    const metrics = metricsCollector.collect()

    if (metrics.memory_heap_used !== undefined) {
      observableResult.observe(memoryHeapUsed, metrics.memory_heap_used, baseAttributes)
    }
    if (metrics.memory_heap_total !== undefined) {
      observableResult.observe(memoryHeapTotal, metrics.memory_heap_total, baseAttributes)
    }
    if (metrics.memory_rss !== undefined) {
      observableResult.observe(memoryRss, metrics.memory_rss, baseAttributes)
    }
    if (metrics.memory_external !== undefined) {
      observableResult.observe(memoryExternal, metrics.memory_external, baseAttributes)
    }
    if (metrics.cpu_percent !== undefined) {
      observableResult.observe(cpuPercent, metrics.cpu_percent, baseAttributes)
    }
    if (metrics.cpu_user_micros !== undefined) {
      observableResult.observe(cpuUserMicros, metrics.cpu_user_micros, baseAttributes)
    }
    if (metrics.cpu_system_micros !== undefined) {
      observableResult.observe(cpuSystemMicros, metrics.cpu_system_micros, baseAttributes)
    }
    if (metrics.event_loop_lag_ms !== undefined) {
      observableResult.observe(eventLoopLag, metrics.event_loop_lag_ms, baseAttributes)
    }
    if (metrics.uptime_seconds !== undefined) {
      observableResult.observe(uptimeSeconds, metrics.uptime_seconds, baseAttributes)
    }
  }

  meter.addBatchObservableCallback(batchCallback, [
    memoryHeapUsed,
    memoryHeapTotal,
    memoryRss,
    memoryExternal,
    cpuPercent,
    cpuUserMicros,
    cpuSystemMicros,
    eventLoopLag,
    uptimeSeconds,
  ])

  registeredGauges = true
}

export function stopWorkerGauges(): void {
  if (metricsCollector) {
    metricsCollector.stopMonitoring()
    metricsCollector = null
  }
  registeredGauges = false
}
