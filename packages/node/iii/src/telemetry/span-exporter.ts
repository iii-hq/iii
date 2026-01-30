/**
 * Span exporter for the III Engine.
 */

import { ExportResultCode, type ExportResult } from '@opentelemetry/core'
import { type ReadableSpan, type SpanExporter } from '@opentelemetry/sdk-trace-base'
import { JsonTraceSerializer } from '@opentelemetry/otlp-transformer'

import { SharedEngineConnection } from './connection'
import { PREFIX_TRACES } from './types'

/**
 * Span exporter using the shared WebSocket connection.
 */
export class EngineSpanExporter implements SpanExporter {
  private static readonly MAX_PENDING_EXPORTS = 100
  private connection: SharedEngineConnection
  private pendingExports: Array<{ spans: ReadableSpan[]; resultCallback?: (result: ExportResult) => void }> = []

  constructor(connection: SharedEngineConnection) {
    this.connection = connection
    this.connection.onConnected(() => this.flushPending())
  }

  private flushPending(): void {
    const pending = this.pendingExports.splice(0, this.pendingExports.length)
    for (const { spans, resultCallback } of pending) {
      this.sendExport(spans, resultCallback)
    }
  }

  private sendExport(spans: ReadableSpan[], resultCallback?: (result: ExportResult) => void): void {
    try {
      const serialized = JsonTraceSerializer.serializeRequest(spans)
      if (!serialized) {
        resultCallback?.({ code: ExportResultCode.SUCCESS })
        return
      }

      this.connection.send(PREFIX_TRACES, serialized, (err) => {
        if (err) {
          console.error('[OTel] Failed to send spans:', err.message)
          resultCallback?.({ code: ExportResultCode.FAILED, error: err })
        } else {
          resultCallback?.({ code: ExportResultCode.SUCCESS })
        }
      })
    } catch (err) {
      console.error('[OTel] Error exporting spans:', err)
      resultCallback?.({ code: ExportResultCode.FAILED, error: err as Error })
    }
  }

  private doExport(spans: ReadableSpan[], resultCallback: (result: ExportResult) => void): void {
    if (this.connection.getState() !== 'connected') {
      if (this.pendingExports.length >= EngineSpanExporter.MAX_PENDING_EXPORTS) {
        const dropped = this.pendingExports.shift()
        dropped?.resultCallback?.({ code: ExportResultCode.FAILED, error: new Error('Queue overflow') })
        console.warn('[OTel] Spans export queue full, dropped oldest entry')
      }
      this.pendingExports.push({ spans, resultCallback })
      // Don't call resultCallback here - it will be called when actually sent or on shutdown
      return
    }

    this.sendExport(spans, resultCallback)
  }

  export(spans: ReadableSpan[], resultCallback: (result: ExportResult) => void): void {
    this.doExport(spans, resultCallback)
  }

  async shutdown(): Promise<void> {
    const pending = this.pendingExports.splice(0, this.pendingExports.length)
    const shutdownError = new Error('Exporter shutdown before export completed')
    for (const { resultCallback } of pending) {
      resultCallback?.({ code: ExportResultCode.FAILED, error: shutdownError })
    }
  }

  async forceFlush(): Promise<void> {
    // No-op, spans are sent immediately
  }
}
