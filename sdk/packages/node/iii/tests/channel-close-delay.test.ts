import { describe, expect, it, vi, beforeEach, afterEach, afterAll } from 'vitest'
import { EventEmitter } from 'node:events'
import type { StreamChannelRef } from '../src/iii-types'

// ---------------------------------------------------------------------------
// Inline mock for the 'ws' module.
// We need a controllable WebSocket stand-in so we can inspect close() calls
// and trigger 'open' events without a real network.
// ---------------------------------------------------------------------------

class MockWebSocket extends EventEmitter {
  static readonly OPEN = 1
  static readonly CLOSED = 3

  readyState = MockWebSocket.OPEN
  closeCode: number | undefined
  closeReason: string | undefined
  terminated = false

  close(code: number, reason: string): void {
    this.closeCode = code
    this.closeReason = reason
    this.readyState = MockWebSocket.CLOSED
  }

  terminate(): void {
    this.terminated = true
    this.readyState = MockWebSocket.CLOSED
  }

  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  send(_data: unknown, callback?: (err?: Error | null) => void): void {
    callback?.()
  }
}

// Replace the 'ws' module with our mock before importing ChannelWriter
vi.mock('ws', () => ({
  WebSocket: MockWebSocket,
}))

// Import ChannelWriter AFTER the mock is registered
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

/**
 * Reach into the private fields of ChannelWriter via `as any` so we can
 * set up pre-conditions (ws, wsReady) without going through the network.
 */
function injectWebSocket(
  writer: InstanceType<typeof ChannelWriter>,
  mockWs: MockWebSocket,
  wsReady: boolean,
): void {
  // biome-ignore lint/suspicious/noExplicitAny: accessing private fields for testing
  const w = writer as any
  w.ws = mockWs
  w.wsReady = wsReady
}

/**
 * Wrap `stream.end()` in a promise that resolves when the 'finish' event fires.
 * The Writable stream emits 'finish' after the `final` callback completes,
 * so this is the reliable way to await stream teardown.
 */
function endStream(stream: import('node:stream').Writable): Promise<void> {
  return new Promise<void>((resolve, reject) => {
    stream.once('finish', resolve)
    stream.once('error', reject)
    stream.end()
  })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('ChannelWriter – 10ms delay before close frame', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
    vi.restoreAllMocks()
  })

  // Restore real timers before the global afterAll in setup.ts runs, so that
  // the SDK shutdown logic (which relies on real setTimeout for reconnects)
  // is not blocked by fake timers.
  afterAll(() => {
    vi.useRealTimers()
  })

  it('calls setTimeout with 10ms delay when ws is ready and stream.end() is called', async () => {
    const writer = new ChannelWriter('ws://localhost:3000', makeRef())
    const mockWs = new MockWebSocket()
    injectWebSocket(writer, mockWs, /* wsReady= */ true)

    const setTimeoutSpy = vi.spyOn(globalThis, 'setTimeout')

    const finishPromise = endStream(writer.stream)

    // The final callback should have scheduled a delayed close
    expect(setTimeoutSpy).toHaveBeenCalledWith(expect.any(Function), 10)

    // Advance timers so the stream can finish
    vi.advanceTimersByTime(10)

    await finishPromise
  })

  it('closes the WebSocket with code 1000 and reason stream_complete after the 10ms delay', async () => {
    const writer = new ChannelWriter('ws://localhost:3000', makeRef())
    const mockWs = new MockWebSocket()
    injectWebSocket(writer, mockWs, /* wsReady= */ true)

    const finishPromise = endStream(writer.stream)

    // Before the timer fires, close must NOT have been called yet
    expect(mockWs.closeCode).toBeUndefined()

    // Advance fake timers past the 10ms threshold
    vi.advanceTimersByTime(10)

    await finishPromise

    expect(mockWs.closeCode).toBe(1000)
    expect(mockWs.closeReason).toBe('stream_complete')
  })

  it('does not call setTimeout when ws is null and calls callback immediately', async () => {
    const writer = new ChannelWriter('ws://localhost:3000', makeRef())
    // Leave ws as null (default state – no ensureConnected called)

    const setTimeoutSpy = vi.spyOn(globalThis, 'setTimeout')

    // When ws is null the final handler exits immediately; 'finish' fires on next tick
    await endStream(writer.stream)

    // The final handler should have returned without scheduling a delayed close
    expect(setTimeoutSpy).not.toHaveBeenCalled()
  })

  it('waits for the open event before scheduling the 10ms delay when ws is not ready', async () => {
    const writer = new ChannelWriter('ws://localhost:3000', makeRef())
    const mockWs = new MockWebSocket()
    injectWebSocket(writer, mockWs, /* wsReady= */ false)

    const setTimeoutSpy = vi.spyOn(globalThis, 'setTimeout')

    const finishPromise = endStream(writer.stream)

    // Before 'open' fires, setTimeout must not have been called
    expect(setTimeoutSpy).not.toHaveBeenCalled()
    expect(mockWs.closeCode).toBeUndefined()

    // Simulate the WebSocket becoming ready
    mockWs.emit('open')

    // Now setTimeout should have been registered with 10ms
    expect(setTimeoutSpy).toHaveBeenCalledWith(expect.any(Function), 10)

    // Advance to let the timer execute
    vi.advanceTimersByTime(10)

    await finishPromise

    expect(mockWs.closeCode).toBe(1000)
    expect(mockWs.closeReason).toBe('stream_complete')
  })

  it('close() method does not use setTimeout – it closes synchronously when ws is ready', () => {
    const writer = new ChannelWriter('ws://localhost:3000', makeRef())
    const mockWs = new MockWebSocket()
    injectWebSocket(writer, mockWs, /* wsReady= */ true)

    const setTimeoutSpy = vi.spyOn(globalThis, 'setTimeout')

    writer.close()

    // The standalone close() method should NOT introduce the 10ms delay
    expect(setTimeoutSpy).not.toHaveBeenCalled()
    // But it should close the socket immediately
    expect(mockWs.closeCode).toBe(1000)
    expect(mockWs.closeReason).toBe('channel_close')
  })

  it('close() method does not use setTimeout when ws is not yet ready', () => {
    const writer = new ChannelWriter('ws://localhost:3000', makeRef())
    const mockWs = new MockWebSocket()
    injectWebSocket(writer, mockWs, /* wsReady= */ false)

    const setTimeoutSpy = vi.spyOn(globalThis, 'setTimeout')

    writer.close()

    // Should register an 'open' listener but never a setTimeout
    expect(setTimeoutSpy).not.toHaveBeenCalled()

    // When the socket opens the close is invoked directly (no delay)
    mockWs.emit('open')

    expect(setTimeoutSpy).not.toHaveBeenCalled()
    expect(mockWs.closeCode).toBe(1000)
    expect(mockWs.closeReason).toBe('channel_close')
  })
})
