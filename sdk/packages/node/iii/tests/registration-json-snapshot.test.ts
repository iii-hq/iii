import { EventEmitter } from 'node:events'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { IIIClient } from '../src/types'

const LIMIT = 16 * 1024 * 1024
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
  triggerTypes: Map<string, Retained>
  triggers: Map<string, Record<string, unknown>>
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

const triggerHandler = () => ({
  registerTrigger: vi.fn(async () => {}),
  unregisterTrigger: vi.fn(async () => {}),
})

describe('Registration JSON snapshots', () => {
  it('replays accepted nested fields independently of caller mutation and preserves handlers', async () => {
    const socket = makeSdk()
    const schema = { type: 'object' as const, properties: { value: { type: 'string' } } }
    const metadata = { nested: { value: 'original' }, nullable: null }
    const config = { nested: { value: 'accepted' } }
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
    const typeHandler = triggerHandler()
    const typeInput = {
      id: 'custom',
      description: 'accepted',
      trigger_request_format: schema,
      call_request_format: schema,
    }
    const typeRef = sdk.registerTriggerType(typeInput, typeHandler)
    sdk.registerTrigger({ type: 'custom', function_id: 'size::handler', config, metadata })
    const functionHandler = sdk.functions.get('size::handler')?.handler
    expect(sdk.triggerTypes.get('custom')?.handler).toBe(typeHandler)
    expect([...sdk.triggers.values()][0].config).not.toBe(config)

    schema.properties.value.type = 'x'.repeat(LIMIT)
    metadata.nested.value = 'mutated'
    config.nested.value = 'x'.repeat(LIMIT)
    headers['X-Test'] = 'mutated'
    auth.token_key = 'mutated'
    typeInput.id = 'mutated'
    typeRef.registerTrigger('size::handler', { extra: true })
    sdk.messagesToSend.push({ type: 'pong' })
    expect(() => socket.simulateOpen()).not.toThrow()

    const registrations = socket.sent.filter(
      frame => String(frame.type).startsWith('register') && frame.type !== 'registerworker',
    )
    const expected = { type: 'object', properties: { value: { type: 'string' } } }
    expect(registrations.find(frame => frame.type === 'registertriggertype')).toMatchObject({
      id: 'custom',
      trigger_request_format: expected,
      call_request_format: expected,
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
    const triggers = registrations.filter(frame => frame.type === 'registertrigger')
    expect(triggers[0]).toMatchObject({
      config: { nested: { value: 'accepted' } },
      metadata: { nested: { value: 'original' } },
    })
    expect(triggers[1]).toMatchObject({ trigger_type: 'custom', config: { extra: true } })
    expect(socket.sent[socket.sent.length - 1]).toEqual({ type: 'pong' })
    expect(sdk.messagesToSend).toHaveLength(0)
    expect(sdk.functions.get('size::handler')?.handler).toBe(functionHandler)
    await (functionHandler as (data: unknown) => Promise<unknown>)({})
    expect(handler).toHaveBeenCalledOnce()
  })

  it('uses existing JSON semantics for toJSON, absent undefined, and explicit null', () => {
    const socket = makeSdk()
    const config = { toJSON: () => ({ value: 'wire', absent: undefined, nullable: null }) }
    sdk.registerTrigger({ type: 'custom', function_id: 'size::handler', config })
    config.toJSON = () => ({ value: 'mutated', absent: undefined, nullable: null })
    socket.simulateOpen()
    expect(socket.sent.find(frame => frame.type === 'registertrigger')?.config).toEqual({
      value: 'wire',
      nullable: null,
    })
  })

  it('rejects replacements before retaining them and keeps accepted ownership', () => {
    const socket = makeSdk()
    const originalHandler = triggerHandler()
    sdk.registerTriggerType({ id: 'custom', description: 'accepted' }, originalHandler)
    const accepted = sdk.triggerTypes.get('custom')
    expect(() =>
      sdk.registerTriggerType({ id: 'custom', description: 'x'.repeat(LIMIT) }, triggerHandler()),
    ).toThrow('payload_too_large')
    expect(sdk.triggerTypes.get('custom')).toBe(accepted)
    expect(sdk.triggerTypes.get('custom')?.handler).toBe(originalHandler)
    expect(() =>
      sdk.registerFunction('size::rejected', async () => {}, {
        metadata: { value: 'x'.repeat(LIMIT) },
      }),
    ).toThrow('payload_too_large')
    expect(sdk.functions.has('size::rejected')).toBe(false)
    expect(() =>
      sdk.registerTrigger({
        type: 'custom',
        function_id: 'size::handler',
        config: { value: 'x'.repeat(LIMIT) },
      }),
    ).toThrow('payload_too_large')
    expect(sdk.triggers.size).toBe(0)
    expect(sdk.messagesToSend).toHaveLength(0)
    socket.simulateOpen()
    expect(socket.sent.find(frame => frame.type === 'registertriggertype')).toMatchObject({
      id: 'custom',
      description: 'accepted',
    })
  })

  it.each([
    'type',
    'function',
    'trigger',
  ])('isolates a permanent %s rejection inside the real open callback', kind => {
    const socket = makeSdk()
    // Simulate an invalid retained/legacy entry independently of caller snapshots.
    sdk.registerTriggerType({ id: 'custom', description: 'accepted' }, triggerHandler())
    sdk.registerFunction('size::handler', async () => ({ ok: true }))
    sdk.registerTrigger({ type: 'custom', function_id: 'size::handler', config: {} })
    if (kind === 'type') sdk.triggerTypes.get('custom')!.message.description = 'x'.repeat(LIMIT)
    if (kind === 'function')
      sdk.functions.get('size::handler')!.message.metadata = { value: 'x'.repeat(LIMIT) }
    if (kind === 'trigger') [...sdk.triggers.values()][0].config = { value: 'x'.repeat(LIMIT) }
    sdk.registerTriggerType({ id: 'later', description: 'small' }, triggerHandler())
    sdk.registerFunction('size::later', async () => ({ ok: true }))
    sdk.registerTrigger({ type: 'later', function_id: 'size::later', config: { value: 1 } })
    sdk.messagesToSend.push({ type: 'pong' })
    expect(() => socket.simulateOpen()).not.toThrow()
    expect(socket.sent).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ type: 'registertriggertype', id: 'later' }),
        expect.objectContaining({ type: 'registerfunction', id: 'size::later' }),
        expect.objectContaining({ type: 'registertrigger', function_id: 'size::later' }),
      ]),
    )
    expect(socket.sent[socket.sent.length - 1]).toEqual({ type: 'pong' })
    expect(sdk.messagesToSend).toHaveLength(0)
  })

  it('does not mask unexpected or transport-related errors thrown by replay', () => {
    const socket = makeSdk()
    sdk.registerTriggerType({ id: 'custom', description: 'accepted' }, triggerHandler())
    sdk.registerWorkerMetadata = vi.fn()
    const error = new Error('transport or unexpected failure')
    vi.spyOn(sdk, 'sendMessage').mockImplementation(() => {
      throw error
    })
    expect(() => socket.simulateOpen()).toThrow(error)
  })
})
