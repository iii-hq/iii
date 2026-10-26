import { EventEmitter } from 'node:events'
import { afterAll, afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// ---------------------------------------------------------------------------
// Inline mock for the 'ws' module, following connection-heartbeat.test.ts.
// Records each socket's URL and constructor options so the handshake headers
// can be asserted without a network.
// ---------------------------------------------------------------------------

const sockets: MockWebSocket[] = []

class MockWebSocket extends EventEmitter {
  static readonly CONNECTING = 0
  static readonly OPEN = 1
  static readonly CLOSING = 2
  static readonly CLOSED = 3

  readyState: number = MockWebSocket.CONNECTING

  constructor(
    public readonly url?: string,
    public readonly opts?: Record<string, unknown>,
  ) {
    super()
    sockets.push(this)
  }

  // Nothing asserts on these; the SDK only needs them to exist.
  send = vi.fn()
  ping = vi.fn()
  resume = vi.fn()

  close(): void {
    this.readyState = MockWebSocket.CLOSED
  }
  terminate(): void {
    this.readyState = MockWebSocket.CLOSED
  }
}

vi.mock('ws', () => ({
  WebSocket: MockWebSocket,
  default: { WebSocket: MockWebSocket },
}))

// setup.ts eagerly imports the SDK (and the real 'ws') before this file's mock
// takes hold, so drop the cached graph and re-import against the mock.
vi.resetModules()
const { registerWorker } = await import('../src/index')
const { ChannelReader, ChannelWriter } = await import('../src/channels')

const ref = { channel_id: 'chan', access_key: 'key', direction: 'write' } as const

function connect(headers?: Record<string, string>) {
  return registerWorker('ws://engine.test', { otel: { enabled: false }, headers })
}

beforeEach(() => {
  sockets.length = 0
  vi.stubEnv('III_TOKEN', undefined as unknown as string)
})

afterEach(() => {
  vi.unstubAllEnvs()
})

afterAll(() => {
  vi.useRealTimers()
})

describe('III_TOKEN on the main connection', () => {
  it('sends the token as a bearer Authorization header', async () => {
    vi.stubEnv('III_TOKEN', 'secret')
    const sdk = connect({ 'x-other': 'kept' })
    try {
      expect(sockets[0].opts?.headers).toEqual({ 'x-other': 'kept', Authorization: 'Bearer secret' })
    } finally {
      await sdk.shutdown()
    }
  })

  it('lets an explicit Authorization header win, whatever its case', async () => {
    vi.stubEnv('III_TOKEN', 'secret')
    const sdk = connect({ authorization: 'Bearer explicit' })
    try {
      expect(sockets[0].opts?.headers).toEqual({ authorization: 'Bearer explicit' })
    } finally {
      await sdk.shutdown()
    }
  })

  it.each([undefined, '', '   '])('adds no header for III_TOKEN=%j', async (value) => {
    vi.stubEnv('III_TOKEN', value as string)
    const sdk = connect()
    try {
      expect(sockets[0].opts?.headers).toBeUndefined()
    } finally {
      await sdk.shutdown()
    }
  })
})

describe('III_TOKEN on telemetry', () => {
  it('hands the main connection headers to the engine /otel socket', async () => {
    vi.stubEnv('III_TOKEN', 'secret')
    const sdk = registerWorker('ws://engine.test', { headers: { 'x-other': 'kept' } })
    try {
      const telemetry = sockets.find((s) => s.url === 'ws://engine.test/otel')
      expect(telemetry?.opts?.headers).toEqual({ 'x-other': 'kept', Authorization: 'Bearer secret' })
    } finally {
      await sdk.shutdown()
    }
  })
})

describe('III_TOKEN on channels', () => {
  it('sends the same bearer header from the writer and the reader', () => {
    vi.stubEnv('III_TOKEN', 'secret')

    new ChannelWriter('ws://engine.test', ref).sendMessage('hi')
    new ChannelReader('ws://engine.test', { ...ref, direction: 'read' }).stream.read()

    expect(sockets.map((s) => s.opts?.headers)).toEqual([
      { Authorization: 'Bearer secret' },
      { Authorization: 'Bearer secret' },
    ])
  })

  it('sends the explicit headers the worker was given', () => {
    vi.stubEnv('III_TOKEN', 'secret')

    new ChannelWriter('ws://engine.test', ref, { Authorization: 'Bearer explicit' }).sendMessage('hi')

    expect(sockets[0].opts?.headers).toEqual({ Authorization: 'Bearer explicit' })
  })

  it('hands a worker its headers on the channels it resolves from an invocation', async () => {
    const sdk = connect({ 'x-other': 'kept' })
    try {
      // biome-ignore lint/suspicious/noExplicitAny: reaching the private resolver the invoke path uses
      ;(sdk as any).resolveChannelValue(ref).sendMessage('hi')
      expect(sockets[1].opts?.headers).toEqual({ 'x-other': 'kept' })
    } finally {
      await sdk.shutdown()
    }
  })

  it('keeps the base query ahead of its own', () => {
    new ChannelWriter('ws://engine.test/?token=abc', ref).sendMessage('hi')
    new ChannelWriter('ws://engine.test/prefix/?token=abc&x=1', { ...ref, access_key: 'a b' }).sendMessage('hi')
    new ChannelWriter('ws://engine.test/', ref).sendMessage('hi')

    expect(sockets.map((s) => s.url)).toEqual([
      'ws://engine.test/ws/channels/chan?token=abc&key=key&dir=write',
      'ws://engine.test/prefix/ws/channels/chan?token=abc&x=1&key=a%20b&dir=write',
      'ws://engine.test/ws/channels/chan?key=key&dir=write',
    ])
  })
})
