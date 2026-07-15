import { EventEmitter } from 'node:events'
import { afterAll, beforeEach, describe, expect, it, vi } from 'vitest'

// ---------------------------------------------------------------------------
// Inline mock for the 'ws' module, following channel-writer-readiness.test.ts.
// Records constructor options so handshakeTimeout can be asserted, counts
// pings, and mirrors the real socket's terminate() -> 'close' transition so
// the Sdk's heartbeat/reconnect wiring can be driven with fake timers.
// ---------------------------------------------------------------------------

const sockets: MockWebSocket[] = []

class MockWebSocket extends EventEmitter {
  static readonly CONNECTING = 0
  static readonly OPEN = 1
  static readonly CLOSING = 2
  static readonly CLOSED = 3

  readyState: number = MockWebSocket.CONNECTING
  readonly sent: Array<Buffer | string> = []
  pings = 0
  terminated = false

  constructor(
    public readonly url?: string,
    public readonly opts?: Record<string, unknown>,
  ) {
    super()
    sockets.push(this)
  }

  send(data: Buffer | string, callback?: (err?: Error | null) => void): void {
    this.sent.push(data)
    callback?.()
  }

  ping(): void {
    this.pings++
  }

  close(): void {
    this.readyState = MockWebSocket.CLOSED
  }

  terminate(): void {
    if (this.terminated) {
      return
    }
    this.terminated = true
    this.readyState = MockWebSocket.CLOSED
    this.emit('close')
  }

  /** Drive the open transition the way the real socket would. */
  simulateOpen(): void {
    this.readyState = MockWebSocket.OPEN
    this.emit('open')
  }
}

vi.mock('ws', () => ({
  WebSocket: MockWebSocket,
  default: { WebSocket: MockWebSocket },
}))

// setup.ts eagerly imports the SDK (and the real 'ws') before this file's mock
// takes hold, so drop the cached graph and re-import against the mock.
// Mirrors logger-runtime.test.ts.
vi.resetModules()
const { registerWorker } = await import('../src/index')

function makeWorker(address: string, headers?: Record<string, string>) {
  return registerWorker(address, {
    otel: { enabled: false },
    headers,
    reconnectionConfig: { initialDelayMs: 10, jitterFactor: 0 },
  })
}

beforeEach(() => {
  sockets.length = 0
})

// Restore real timers before the global afterAll in setup.ts runs, so the SDK
// shutdown logic (which relies on real setTimeout) is not blocked.
afterAll(() => {
  vi.useRealTimers()
})

describe('Sdk – WebSocket handshake timeout and keepalive heartbeat', () => {
  it('passes handshakeTimeout (and headers) to the WebSocket constructor', async () => {
    const sdk = makeWorker('ws://heartbeat-opts.test', { a: 'b' })
    try {
      expect(sockets[0].opts).toMatchObject({ handshakeTimeout: 10000 })
      expect(sockets[0].opts?.headers).toEqual({ a: 'b' })
    } finally {
      await sdk.shutdown()
    }
  })

  it('pings every 20s and stays connected while pongs keep arriving', async () => {
    vi.useFakeTimers()
    const sdk = makeWorker('ws://heartbeat-ping.test')
    try {
      const sock = sockets[0]
      sock.simulateOpen()

      await vi.advanceTimersByTimeAsync(20_000)
      expect(sock.pings).toBe(1)
      sock.emit('pong')

      await vi.advanceTimersByTimeAsync(20_000)
      expect(sock.pings).toBe(2)
      sock.emit('pong')

      // 60s of wall time has passed, but the deadline was kept fresh.
      await vi.advanceTimersByTimeAsync(20_000)
      expect(sock.pings).toBe(3)
      expect(sock.terminated).toBe(false)
    } finally {
      vi.useRealTimers()
      await sdk.shutdown()
    }
  })

  it('terminates after 60s without inbound frames and reconnects', async () => {
    vi.useFakeTimers()
    const sdk = makeWorker('ws://heartbeat-idle.test')
    try {
      const sock = sockets[0]
      sock.simulateOpen()

      // Ticks at 20s/40s ping; the 60s tick crosses the idle deadline.
      await vi.advanceTimersByTimeAsync(60_000)
      expect(sock.pings).toBe(2)
      expect(sock.terminated).toBe(true)

      // terminate() emitted 'close' -> backoff (10ms, jitter 0) -> fresh socket.
      await vi.advanceTimersByTimeAsync(20)
      expect(sockets.length).toBe(2)
      expect(sockets[1]).not.toBe(sock)
    } finally {
      vi.useRealTimers()
      await sdk.shutdown()
    }
  })

  it('any inbound message resets the idle deadline', async () => {
    vi.useFakeTimers()
    const sdk = makeWorker('ws://heartbeat-touch.test')
    try {
      const sock = sockets[0]
      sock.simulateOpen()

      await vi.advanceTimersByTimeAsync(40_000)
      sock.emit('message', Buffer.from('{"type":"noop"}'))

      // t=60s: only 20s since the last inbound frame — still alive.
      await vi.advanceTimersByTimeAsync(20_000)
      expect(sock.terminated).toBe(false)

      // t=100s: 60s since the last inbound frame — terminated.
      await vi.advanceTimersByTimeAsync(40_000)
      expect(sock.terminated).toBe(true)
    } finally {
      vi.useRealTimers()
      await sdk.shutdown()
    }
  })
})
