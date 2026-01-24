/**
 * Metrics exporter for the III Engine.
 */

import { ExportResultCode, type ExportResult } from '@opentelemetry/core'
import { type PushMetricExporter, type ResourceMetrics } from '@opentelemetry/sdk-metrics'
import { JsonMetricsSerializer } from '@opentelemetry/otlp-transformer'

import { SharedEngineConnection } from './connection'
import { PREFIX_METRICS } from './types'

/**
 * Metrics exporter using the shared WebSocket connection.
 */
export class EngineMetricsExporter implements PushMetricExporter {
  private static readonly MAX_PENDING_EXPORTS = 100
  private connection: SharedEngineConnection
  private pendingExports: Array<{ metrics: ResourceMetrics }> = []

  constructor(connection: SharedEngineConnection) {
    this.connection = connection
    this.connection.onConnected(() => this.flushPending())
  }

  private flushPending(): void {
    const pending = this.pendingExports.splice(0, this.pendingExports.length)
    for (const { metrics } of pending) {
      this.sendExport(metrics)
    }
  }

  private sendExport(metricsData: ResourceMetrics, resultCallback?: (result: ExportResult) => void): void {
    try {
      const serialized = JsonMetricsSerializer.serializeRequest(metricsData)
      if (!serialized) {
        resultCallback?.({ code: ExportResultCode.SUCCESS })
        return
      }

      this.connection.send(PREFIX_METRICS, serialized, (err) => {
        if (err) {
          console.error('[OTel] Failed to send metrics:', err.message)
          resultCallback?.({ code: ExportResultCode.FAILED, error: err })
        } else {
          resultCallback?.({ code: ExportResultCode.SUCCESS })
        }
      })
    } catch (err) {
      console.error('[OTel] Error exporting metrics:', err)
      resultCallback?.({ code: ExportResultCode.FAILED, error: err as Error })
    }
  }

  private doExport(metricsData: ResourceMetrics, resultCallback: (result: ExportResult) => void): void {
    if (this.connection.getState() !== 'connected') {
      if (this.pendingExports.length >= EngineMetricsExporter.MAX_PENDING_EXPORTS) {
        this.pendingExports.shift()
        console.warn('[OTel] Metrics export queue full, dropped oldest entry')
      }
      this.pendingExports.push({ metrics: metricsData })
      resultCallback({ code: ExportResultCode.SUCCESS })
      return
    }

    this.sendExport(metricsData, resultCallback)
  }

  export(metrics: ResourceMetrics, resultCallback: (result: ExportResult) => void): void {
    this.doExport(metrics, resultCallback)
  }

  async shutdown(): Promise<void> {
    this.pendingExports = []
  }

  async forceFlush(): Promise<void> {
    // No-op
  }
}
