import { ExportResultCode } from '@opentelemetry/core'
import { describe, expect, it, vi } from 'vitest'
import type { SharedEngineConnection } from './connection'
import { EngineSpanExporter } from './span-exporter'
import type { ConnectionState } from './types'

/** Minimal connection stub so exporter behavior can be driven without a socket. */
function stubConnection(state: ConnectionState) {
  const stub = {
    state,
    shuttingDown: false,
    getState: () => stub.state,
    isShuttingDown: () => stub.shuttingDown,
    onConnected: () => {},
    onFailed: () => {},
    send: () => {},
  }
  return stub
}

describe('EngineSpanExporter shutdown behavior', () => {
  it('queues exports while disconnected but fails fast once shutting down', () => {
    const connection = stubConnection('disconnected')
    const exporter = new EngineSpanExporter(connection as unknown as SharedEngineConnection)

    // Disconnected but still running: the export is queued for a later reconnect,
    // so the callback stays pending.
    const queued = vi.fn()
    exporter.export([], queued)
    expect(queued).not.toHaveBeenCalled()

    // Shutting down with no live connection: the export fails fast instead of
    // queueing, so a final forceFlush() can't hang waiting for a reconnect that
    // will never happen (regression test for the shutdownOtel() hang).
    connection.shuttingDown = true
    const failed = vi.fn()
    exporter.export([], failed)
    expect(failed).toHaveBeenCalledOnce()
    expect(failed.mock.calls[0][0].code).toBe(ExportResultCode.FAILED)
  })
})
