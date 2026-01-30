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
  private pendingExports: Array<{ metrics: ResourceMetrics; resultCallback?: (result: ExportResult) => void }> = []

  constructor(connection: SharedEngineConnection) {
    this.connection = connection
    this.connection.onConnected(() => this.flushPending())
  }

  private flushPending(): void {
    const pending = this.pendingExports.splice(0, this.pendingExports.length)
    for (const { metrics, resultCallback } of pending) {
      this.sendExport(metrics, resultCallback)
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
        const dropped = this.pendingExports.shift()
        dropped?.resultCallback?.({ code: ExportResultCode.FAILED, error: new Error('Queue overflow') })
        console.warn('[OTel] Metrics export queue full, dropped oldest entry')
      }
      this.pendingExports.push({ metrics: metricsData, resultCallback })
      // Don't call resultCallback here - it will be called when actually sent or on shutdown
      return
    }

    this.sendExport(metricsData, resultCallback)
  }

  export(metrics: ResourceMetrics, resultCallback: (result: ExportResult) => void): void {
    this.doExport(metrics, resultCallback)
  }

  async shutdown(): Promise<void> {
    const pending = this.pendingExports.splice(0, this.pendingExports.length)
    const shutdownError = new Error('Exporter shutdown before export completed')
    for (const { resultCallback } of pending) {
      resultCallback?.({ code: ExportResultCode.FAILED, error: shutdownError })
    }
  }

  async forceFlush(): Promise<void> {
    // No-op
  }
}
