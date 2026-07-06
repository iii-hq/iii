import { describe, expect, it, vi, afterAll } from 'vitest'
import { EventEmitter } from 'node:events'
import type { StreamChannelRef } from '../src/iii-types'

// ---------------------------------------------------------------------------
// Inline mock for the 'ws' module.
//
// Unlike the mock in channel-close-delay.test.ts, this one models the full
// readyState lifecycle (CONNECTING -> OPEN -> CLOSED), records everything sent
// so we can assert a dead socket is never touched, and can simulate a send()
// that throws — the exact failures the readyState guard + try/catch protect
// against. Returning both the named and default export prevents the real 'ws'
// from leaking through and opening a live connection (see otel-exception.test).
// ---------------------------------------------------------------------------

class MockWebSocket extends EventEmitter {
  static readonly CONNECTING = 0
  static readonly OPEN = 1
  static readonly CLOSING = 2
  static readonly CLOSED = 3

  // Real sockets start life CONNECTING; 'open' must be simulated explicitly.
  readyState: number = MockWebSocket.CONNECTING
  readonly sent: Array<Buffer | string> = []
  throwOnSend = false
  closeCode: number | undefined
  closeReason: string | undefined
  terminated = false

  constructor(public readonly url?: string) {
    super()
  }

  send(data: Buffer | string, callback?: (err?: Error | null) => void): void {
    if (this.throwOnSend) {
      throw new Error('WebSocket is not open: readyState 3 (CLOSED)')
    }
    this.sent.push(data)
    callback?.()
  }

  close(code?: number, reason?: string): void {
    this.closeCode = code
    this.closeReason = reason
    this.readyState = MockWebSocket.CLOSED
  }

  terminate(): void {
    this.terminated = true
    this.readyState = MockWebSocket.CLOSED
  }

  /** Drive the open transition the way the real socket would. */
  simulateOpen(): void {
    this.readyState = MockWebSocket.OPEN
    this.emit('open')
  }

  /** Simulate the server/network dropping the connection. */
  simulateClose(): void {
    this.readyState = MockWebSocket.CLOSED
    this.emit('close')
  }

  /** Simulate a transport error. */
  simulateError(err: Error): void {
    this.readyState = MockWebSocket.CLOSED
    this.emit('error', err)
  }
}

vi.mock('ws', () => ({
  WebSocket: MockWebSocket,
  default: { WebSocket: MockWebSocket },
}))

// setup.ts eagerly imports the SDK (and the real 'ws') before this file's mock
// takes hold, so drop the cached graph and re-import channels.ts against the
// mock. Mirrors logger-runtime.test.ts.
vi.resetModules()
const { ChannelWriter } = await import('../src/channels')

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeRef(overrides?: Partial<StreamChannelRef>): StreamChannelRef {
  return {
    channel_id: 'test-channel-id',
    access_key: 'test-access-key',
    direction: 'write',
    ...overrides,
  }
}

/** Private fields the tests reach into (matches channel-close-delay.test.ts). */
interface WriterInternals {
  ws: MockWebSocket | null
  wsReady: boolean
  pendingMessages: { data: Buffer | string; callback: (err?: Error | null) => void }[]
  ensureConnected(): void
}

function internals(writer: InstanceType<typeof ChannelWriter>): WriterInternals {
  // biome-ignore lint/suspicious/noExplicitAny: accessing private fields for testing
  return writer as any
}

function injectWebSocket(
  writer: InstanceType<typeof ChannelWriter>,
  mockWs: MockWebSocket,
  wsReady: boolean,
): void {
  const w = internals(writer)
  w.ws = mockWs
  w.wsReady = wsReady
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// Restore real timers before the global afterAll in setup.ts runs, so the SDK
// shutdown logic (which relies on real setTimeout) is not blocked.
afterAll(() => {
  vi.useRealTimers()
})

describe('ChannelWriter – WebSocket readiness race', () => {
  it('never sends on a dead socket even when wsReady is stuck true; it reconnects and queues', () => {
    const writer = new ChannelWriter('ws://localhost:3000', makeRef())
    const w = internals(writer)

    // A socket that has died, with the readiness flag stuck true — the exact
    // state the old `if (this.wsReady && this.ws)` guard mishandled.
    const dead = new MockWebSocket()
    dead.readyState = MockWebSocket.CLOSED
    injectWebSocket(writer, dead, /* wsReady= */ true)

    writer.sendMessage('after-drop')

    // The dead socket must never be sent on (the old code would have thrown
    // here on a real socket, or written into the void).
    expect(dead.sent).toEqual([])
    // ensureConnected replaced the dead socket with a fresh one...
    expect(w.ws).not.toBe(dead)
    expect(w.ws).toBeInstanceOf(MockWebSocket)
    // ...and the message is queued against it (the new socket is still CONNECTING).
    expect(w.pendingMessages.length).toBe(1)
  })

  it('requeues instead of throwing when send() throws on a socket that died underneath us', () => {
    const writer = new ChannelWriter('ws://localhost:3000', makeRef())
    const w = internals(writer)

    // readyState reports OPEN but the underlying send throws — the classic
    // "died between the check and the call" race. try/catch must catch it.
    const flaky = new MockWebSocket()
    flaky.readyState = MockWebSocket.OPEN
    flaky.throwOnSend = true
    injectWebSocket(writer, flaky, /* wsReady= */ true)

    expect(() => writer.sendMessage('boom')).not.toThrow()
    expect(w.pendingMessages.length).toBe(1)
  })

  it("resets wsReady to false when the socket emits 'close'", () => {
    const writer = new ChannelWriter('ws://localhost:3000', makeRef())
    const w = internals(writer)

    w.ensureConnected()
    const sock = w.ws as MockWebSocket
    sock.simulateOpen()
    expect(w.wsReady).toBe(true)

    sock.simulateClose()
    expect(w.wsReady).toBe(false)
  })

  it("resets wsReady to false when the socket emits 'error'", () => {
    const writer = new ChannelWriter('ws://localhost:3000', makeRef())
    const w = internals(writer)
    // The error handler destroys the stream with the error; swallow it so the
    // Writable does not raise an unhandled 'error'.
    writer.stream.on('error', () => {})

    w.ensureConnected()
    const sock = w.ws as MockWebSocket
    sock.simulateOpen()
    expect(w.wsReady).toBe(true)

    sock.simulateError(new Error('transport failure'))
    expect(w.wsReady).toBe(false)
  })

  it('ensureConnected reuses a live socket but replaces a dead one', () => {
    const writer = new ChannelWriter('ws://localhost:3000', makeRef())
    const w = internals(writer)

    w.ensureConnected()
    const first = w.ws as MockWebSocket

    // Still CONNECTING → reused.
    w.ensureConnected()
    expect(w.ws).toBe(first)

    // OPEN → reused.
    first.simulateOpen()
    w.ensureConnected()
    expect(w.ws).toBe(first)

    // Dead → replaced.
    first.readyState = MockWebSocket.CLOSED
    w.ensureConnected()
    expect(w.ws).not.toBe(first)
  })

  it('reconnects after a drop and flushes messages queued against the dead socket', () => {
    const writer = new ChannelWriter('ws://localhost:3000', makeRef())
    const w = internals(writer)
    writer.stream.on('error', () => {})

    // Establish and open the first socket, then send.
    writer.sendMessage('first')
    const sock1 = w.ws as MockWebSocket
    sock1.simulateOpen()
    expect(sock1.sent).toEqual(['first'])
    expect(w.pendingMessages.length).toBe(0)

    // The connection drops.
    sock1.simulateClose()
    expect(w.wsReady).toBe(false)

    // A send after the drop must not touch the dead socket; it reconnects and
    // queues against the fresh socket.
    writer.sendMessage('second')
    const sock2 = w.ws as MockWebSocket
    expect(sock2).not.toBe(sock1)
    expect(sock1.sent).toEqual(['first'])
    expect(w.pendingMessages.length).toBe(1)

    // Once the new socket opens, the queued message flushes.
    sock2.simulateOpen()
    expect(sock2.sent).toEqual(['second'])
    expect(w.pendingMessages.length).toBe(0)
  })
})
