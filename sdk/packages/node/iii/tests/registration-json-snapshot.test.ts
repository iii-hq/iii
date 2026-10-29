import { EventEmitter } from 'node:events'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { IIIClient } from '../src/types'

const LIMIT = 64 * 1024 * 1024
const sockets: MockWebSocket[] = []

class MockWebSocket extends EventEmitter {
  static readonly OPEN = 1
  readyState = 0
  readonly sent: Record<string, unknown>[] = []

  constructor() {
    super()
    sockets.push(this)
  }

  send(data: string, callback?: (error?: Error) => void): void {
    this.sent.push(JSON.parse(data))
    callback?.()
  }

  ping(): void {}
  close(): void {
    this.readyState = 3
  }
  terminate(): void {
    this.readyState = 3
  }
  simulateOpen(): void {
    this.readyState = MockWebSocket.OPEN
    this.emit('open')
  }
}

vi.mock('ws', () => ({ WebSocket: MockWebSocket, default: { WebSocket: MockWebSocket } }))
vi.resetModules()
const { registerWorker } = await import('../src/iii')

type Retained = { message: Record<string, unknown>; handler?: unknown }
type Internal = IIIClient & {
  functions: Map<string, Retained>
  messagesToSend: Record<string, unknown>[]
  registerWorkerMetadata: () => void
  sendMessage: (...args: unknown[]) => void
}
let sdk: Internal
function makeSdk() {
  sdk = registerWorker('ws://snapshot.test', {
    otel: { enabled: false },
    enableMetricsReporting: false,
    reconnectionConfig: { maxRetries: 0 },
  }) as Internal
  return sockets[sockets.length - 1] as MockWebSocket
}
afterEach(async () => {
  vi.restoreAllMocks()
  await sdk.shutdown()
})

describe('Registration JSON snapshots', () => {
  it('replays accepted nested fields independently of caller mutation and preserves handlers', async () => {
    const socket = makeSdk()
    const schema = { type: 'object' as const, properties: { value: { type: 'string' } } }
    const metadata = { nested: { value: 'original' }, nullable: null }
    const argsFormat = { type: 'object' as const, properties: { table: { type: 'string' } } }
    const headers = { 'X-Test': 'original' }
    const auth = { type: 'bearer' as const, token_key: 'original' }
    const handler = vi.fn(async () => ({ ok: true }))
    sdk.registerFunction('size::handler', handler, {
      request_format: schema,
      response_format: schema,
      metadata,
    })
    sdk.registerFunction(
      'size::http',
      { url: 'https://example.invalid/handler', headers, auth },
      { metadata },
    )
    sdk.registerFunction('size::source', (event: unknown) => event, { args_format: argsFormat, private: true })
    const functionHandler = sdk.functions.get('size::handler')?.handler

    schema.properties.value.type = 'x'.repeat(LIMIT)
    metadata.nested.value = 'mutated'
    argsFormat.properties.table.type = 'x'.repeat(LIMIT)
    headers['X-Test'] = 'mutated'
    auth.token_key = 'mutated'
    sdk.messagesToSend.push({ type: 'pong' })
    expect(() => socket.simulateOpen()).not.toThrow()

    socket.emit('message', JSON.stringify({ type: 'workerregistered', worker_id: 'w-1', features: ['observe.v2'] }))

    const registrations = socket.sent.filter(frame => frame.type === 'registerfunction')
    const expected = { type: 'object', properties: { value: { type: 'string' } } }
    expect(registrations.find(frame => frame.id === 'size::source')).toMatchObject({
      args_format: { type: 'object', properties: { table: { type: 'string' } } },
      private: true,
    })
    expect(registrations.find(frame => frame.id === 'size::handler')).toMatchObject({
      request_format: expected,
      response_format: expected,
      metadata: { nested: { value: 'original' }, nullable: null },
    })
    expect(registrations.find(frame => frame.id === 'size::http')).toMatchObject({
      invocation: {
        headers: { 'X-Test': 'original' },
        auth: { type: 'bearer', token_key: 'original' },
      },
    })
    expect(socket.sent.some(frame => frame.type === 'pong')).toBe(true)
    expect(sdk.messagesToSend).toHaveLength(0)
    expect(sdk.functions.get('size::handler')?.handler).toBe(functionHandler)
    await (functionHandler as (data: unknown) => Promise<unknown>)({})
    expect(handler).toHaveBeenCalledOnce()
  })

  it('uses existing JSON semantics for toJSON, absent undefined, and explicit null', () => {
    const socket = makeSdk()
    const metadata = { toJSON: () => ({ value: 'wire', absent: undefined, nullable: null }) }
    sdk.registerFunction('size::handler', async () => null, { metadata: metadata as never })
    metadata.toJSON = () => ({ value: 'mutated', absent: undefined, nullable: null })
    socket.simulateOpen()
    expect(socket.sent.find(frame => frame.type === 'registerfunction')?.metadata).toEqual({
      value: 'wire',
      nullable: null,
    })
  })

  it('rejects replacements before retaining them and keeps accepted ownership', () => {
    const socket = makeSdk()
    const original = async () => ({ ok: true })
    sdk.registerFunction('size::accepted', original)
    const accepted = sdk.functions.get('size::accepted')
    expect(() =>
      sdk.registerFunction('size::rejected', async () => {}, {
        metadata: { value: 'x'.repeat(LIMIT) },
      }),
    ).toThrow('payload_too_large')
    expect(sdk.functions.has('size::rejected')).toBe(false)
    expect(() =>
      sdk.registerFunction('size::rejected', async () => {}, {
        private: true,
        args_format: { description: 'x'.repeat(LIMIT) },
      }),
    ).toThrow('payload_too_large')
    expect(sdk.functions.has('size::rejected')).toBe(false)
    expect(sdk.functions.get('size::accepted')).toBe(accepted)
    expect(sdk.messagesToSend).toHaveLength(0)
    socket.simulateOpen()
    expect(socket.sent.filter(frame => frame.type === 'registerfunction').map(frame => frame.id)).toEqual([
      'size::accepted',
    ])
  })

  it('isolates a permanent function rejection inside the real open callback', () => {
    const socket = makeSdk()
    sdk.registerFunction('size::handler', async () => ({ ok: true }))
    // Simulate an invalid retained entry independently of caller snapshots.
    sdk.functions.get('size::handler')!.message.metadata = { value: 'x'.repeat(LIMIT) }
    sdk.registerFunction('size::later', async () => ({ ok: true }))
    sdk.messagesToSend.push({ type: 'pong' })
    expect(() => socket.simulateOpen()).not.toThrow()
    expect(socket.sent).toEqual(
      expect.arrayContaining([expect.objectContaining({ type: 'registerfunction', id: 'size::later' })]),
    )
    expect(socket.sent[socket.sent.length - 1]).toEqual({ type: 'pong' })
    expect(sdk.messagesToSend).toHaveLength(0)
  })

  it('does not mask unexpected or transport-related errors thrown by replay', () => {
    const socket = makeSdk()
    sdk.registerFunction('size::handler', async () => ({ ok: true }))
    sdk.registerWorkerMetadata = vi.fn()
    const error = new Error('transport or unexpected failure')
    vi.spyOn(sdk, 'sendMessage').mockImplementation(() => {
      throw error
    })
    expect(() => socket.simulateOpen()).toThrow(error)
  })
})
