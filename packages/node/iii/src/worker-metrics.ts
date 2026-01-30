/**
 * Worker metrics collection for the III Node SDK.
 *
 * Collects CPU, memory, and event loop metrics for worker health monitoring.
 * Uses the Node.js built-in `monitorEventLoopDelay` API for accurate
 * event loop lag measurements.
 */

import { type IntervalHistogram, monitorEventLoopDelay, performance } from 'node:perf_hooks'

/**
 * Worker metrics data structure used internally for OTEL metric collection.
 */
export type WorkerMetrics = {
  memory_heap_used?: number
  memory_heap_total?: number
  memory_rss?: number
  memory_external?: number
  cpu_user_micros?: number
  cpu_system_micros?: number
  cpu_percent?: number
  event_loop_lag_ms?: number
  uptime_seconds?: number
  timestamp_ms: number
  runtime: string
}

/**
 * Configuration options for the WorkerMetricsCollector.
 */
export interface WorkerMetricsCollectorOptions {
  /**
   * Event loop delay histogram resolution in milliseconds.
   * Lower values provide more accurate measurements but use more resources.
   * @default 20
   */
  eventLoopResolutionMs?: number
}

/**
 * Collects worker resource metrics including CPU, memory, and event loop lag.
 *
 * Uses the Node.js `monitorEventLoopDelay` API for high-precision event loop
 * delay measurements instead of manual `setImmediate` timing.
 *
 * @example
 * ```typescript
 * const collector = new WorkerMetricsCollector()
 *
 * // Collect metrics periodically
 * setInterval(() => {
 *   const metrics = collector.collect()
 *   console.log('CPU:', metrics.cpu_percent, '%')
 *   console.log('Event Loop Lag:', metrics.event_loop_lag_ms, 'ms')
 * }, 5000)
 *
 * // Clean up when done
 * collector.stopMonitoring()
 * ```
 */
export class WorkerMetricsCollector {
  private readonly startTime: number
  private lastCpuUsage: NodeJS.CpuUsage
  private lastCpuTime: number
  private eventLoopHistogram: IntervalHistogram | null = null

  /**
   * Creates a new WorkerMetricsCollector instance.
   *
   * @param options - Configuration options
   */
  constructor(options: WorkerMetricsCollectorOptions = {}) {
    this.startTime = Date.now()
    this.lastCpuUsage = process.cpuUsage()
    this.lastCpuTime = performance.now()
    this.startEventLoopMonitoring(options.eventLoopResolutionMs ?? 20)
  }

  /**
   * Starts the event loop delay histogram monitoring.
   *
   * @param resolutionMs - Histogram resolution in milliseconds
   */
  private startEventLoopMonitoring(resolutionMs: number): void {
    // Sanitize resolution: must be a positive finite number, minimum 1ms
    const safeResolutionMs =
      Number.isFinite(resolutionMs) && resolutionMs > 0
        ? Math.max(1, Math.floor(resolutionMs))
        : 20 // Default fallback

    this.eventLoopHistogram = monitorEventLoopDelay({ resolution: safeResolutionMs })
    this.eventLoopHistogram.enable()
  }

  /**
   * Stops the event loop monitoring and releases resources.
   * Should be called when the collector is no longer needed.
   */
  public stopMonitoring(): void {
    if (this.eventLoopHistogram) {
      this.eventLoopHistogram.disable()
      this.eventLoopHistogram = null
    }
  }

  /**
   * Collects current worker metrics.
   *
   * This method calculates CPU usage since the last collection,
   * reads memory usage, and gets event loop delay statistics.
   * The event loop histogram is reset after each collection for
   * accurate per-interval measurements.
   *
   * @returns Current worker metrics snapshot
   */
  collect(): WorkerMetrics {
    const memoryUsage = process.memoryUsage()
    const cpuUsage = process.cpuUsage()
    const now = performance.now()

    // Calculate CPU percentage since last collection
    const cpuDelta = {
      user: cpuUsage.user - this.lastCpuUsage.user,
      system: cpuUsage.system - this.lastCpuUsage.system,
    }
    const timeDelta = (now - this.lastCpuTime) * 1000 // Convert ms to microseconds
    const cpuPercent = timeDelta > 0 ? ((cpuDelta.user + cpuDelta.system) / timeDelta) * 100 : 0

    // Update state for next collection
    this.lastCpuUsage = cpuUsage
    this.lastCpuTime = now

    // Get event loop lag from histogram (in nanoseconds, convert to ms)
    let eventLoopLagMs = 0
    if (this.eventLoopHistogram) {
      // Mean is in nanoseconds, convert to milliseconds
      eventLoopLagMs = this.eventLoopHistogram.mean / 1_000_000
      // Reset histogram for next collection interval
      this.eventLoopHistogram.reset()
    }

    return {
      memory_heap_used: memoryUsage.heapUsed,
      memory_heap_total: memoryUsage.heapTotal,
      memory_rss: memoryUsage.rss,
      memory_external: memoryUsage.external,
      cpu_user_micros: cpuUsage.user,
      cpu_system_micros: cpuUsage.system,
      cpu_percent: Math.min(cpuPercent, 100), // Cap at 100%
      event_loop_lag_ms: eventLoopLagMs,
      uptime_seconds: Math.floor((Date.now() - this.startTime) / 1000),
      timestamp_ms: Date.now(),
      runtime: 'node',
    }
  }
}
