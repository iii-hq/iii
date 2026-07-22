import { EventEmitter } from 'node:events'
import { afterAll, beforeEach, describe, expect, it, vi } from 'vitest'

// Inline mock for the 'ws' module (same shape as connection-heartbeat.test.ts):
// records sent frames and mirrors terminate() -> 'close' so the reconnect path
// can be driven with fake timers.
const sockets: MockWebSocket[] = []

class MockWebSocket extends EventEmitter {
  static readonly CONNECTING = 0
  static readonly OPEN = 1
  static readonly CLOSING = 2
  static readonly CLOSED = 3

  readyState: number = MockWebSocket.CONNECTING
  readonly sent: Array<Buffer | string> = []
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

  ping(): void {}

  close(): void {
    this.readyState = MockWebSocket.CLOSED
  }

  terminate(): void {
    if (this.terminated) return
    this.terminated = true
    this.readyState = MockWebSocket.CLOSED
    this.emit('close')
  }

  simulateOpen(): void {
    this.readyState = MockWebSocket.OPEN
    this.emit('open')
  }

  sentMessages(): Array<Record<string, unknown>> {
    return this.sent.map((s) => JSON.parse(s.toString()) as Record<string, unknown>)
  }
}

vi.mock('ws', () => ({
  WebSocket: MockWebSocket,
  default: { WebSocket: MockWebSocket },
}))

vi.resetModules()
const { registerWorker } = await import('../src/index')

function makeWorker(address: string) {
  return registerWorker(address, {
    otel: { enabled: false },
    reconnectionConfig: { initialDelayMs: 10, jitterFactor: 0 },
  })
}

beforeEach(() => {
  sockets.length = 0
})

afterAll(() => {
  vi.useRealTimers()
})

describe('Sdk – reconnect reattach handshake', () => {
  it('sends reattach with the prior worker id before replaying registrations on reconnect', async () => {
    vi.useFakeTimers()
    const sdk = makeWorker('ws://reattach.test')
    try {
      sdk.registerFunction('reattach::fn', async () => ({ ok: true }))

      // First connect: registrations replay, but NO reattach (no prior id yet).
      const sock0 = sockets[0]
      sock0.simulateOpen()
      expect(sock0.sentMessages().some((m) => m.type === 'reattach')).toBe(false)

      // Engine assigns this connection an identity + reattach secret.
      sock0.emit(
        'message',
        JSON.stringify({ type: 'workerregistered', worker_id: 'w-1', reattach_token: 'tok-1' }),
      )

      // Drop the socket; the SDK reconnects.
      sock0.terminate()
      await vi.advanceTimersByTimeAsync(50)

      const sock1 = sockets[1]
      expect(sock1).toBeDefined()
      sock1.simulateOpen()

      const sent1 = sock1.sentMessages()
      const reattachIdx = sent1.findIndex((m) => m.type === 'reattach')
      const registerIdx = sent1.findIndex((m) => typeof m.type === 'string' && m.type.startsWith('register'))

      expect(reattachIdx).toBeGreaterThanOrEqual(0)
      expect(sent1[reattachIdx].previous_worker_id).toBe('w-1')
      expect(sent1[reattachIdx].reattach_token).toBe('tok-1')
      expect(registerIdx).toBeGreaterThanOrEqual(0)
      expect(reattachIdx).toBeLessThan(registerIdx)
    } finally {
      vi.useRealTimers()
      await sdk.shutdown()
    }
  })
})
