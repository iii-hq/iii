import { EventEmitter } from 'node:events'
import { afterEach, describe, expect, it, vi } from 'vitest'

// Records each socket's constructor options so the handshake headers can be
// asserted without a network.
const sockets: MockWebSocket[] = []

class MockWebSocket extends EventEmitter {
  static readonly OPEN = 1

  readyState = 0

  constructor(
    public readonly url: string,
    public readonly opts: { headers?: Record<string, string> },
  ) {
    super()
    sockets.push(this)
  }

  close = vi.fn()
}

vi.mock('ws', () => ({ WebSocket: MockWebSocket }))

// The mock factory runs when 'ws' is first imported, so import the connection
// only after MockWebSocket is defined.
const { SharedEngineConnection } = await import('./connection')

afterEach(() => {
  sockets.length = 0
  vi.useRealTimers()
})

describe('SharedEngineConnection handshake headers', () => {
  it('sends the configured headers on connect and on every reconnect', async () => {
    vi.useFakeTimers()
    const headers = { Authorization: 'Bearer secret' }
    const connection = new SharedEngineConnection(
      'ws://engine.test/otel',
      { initialDelayMs: 1, jitterFactor: 0 },
      headers,
    )
    expect(sockets[0].opts.headers).toEqual(headers)

    sockets[0].emit('close')
    await vi.advanceTimersByTimeAsync(10)

    expect(sockets).toHaveLength(2)
    expect(sockets[1].opts.headers).toEqual(headers)
    await connection.shutdown()
  })
})

describe('SharedEngineConnection shutdown', () => {
  it('stays silent when the socket closes after shutdown() resolved', async () => {
    const connection = new SharedEngineConnection('ws://engine.test/otel')
    const [socket] = sockets
    const log = vi.spyOn(console, 'log').mockImplementation(() => {})
    const error = vi.spyOn(console, 'error').mockImplementation(() => {})

    try {
      await connection.shutdown()
      expect(socket.close).toHaveBeenCalledOnce()
      log.mockClear()

      // ws emits these once the close handshake (or an aborted connect) settles
      socket.emit('error', new Error('WebSocket was closed before the connection was established'))
      socket.emit('close')

      expect(log).not.toHaveBeenCalled()
      expect(error).not.toHaveBeenCalled()
      expect(connection.getState()).toBe('disconnected')
    } finally {
      log.mockRestore()
      error.mockRestore()
    }
  })
})
