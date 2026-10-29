import { afterEach, describe, expect, it, vi } from 'vitest'
import { registerWorker } from '../src/iii'
import { MessageType } from '../src/iii-types'
import type { ISdk } from '../src/types'
import { MockEngine, type MockWebSocket } from './mock-websocket'

const LIMIT = 16 * 1024 * 1024
const LINK = 'https://iii.dev/docs/creating-workers/channels#payload-size'
const message = (data: string) => ({ type: MessageType.InvokeFunction, data })

type Internal = ISdk & {
  prepareJson: (value: Record<string, unknown>) => string
  sendMessageRaw: (data: string) => void
  messagesToSend: Record<string, unknown>[]
  invocations: Map<string, unknown>
  onSocketOpen: () => void
  workerId?: string
  reattachToken?: string
  ws?: WebSocket
  functions: Map<string, { message: { metadata?: unknown }; handler?: unknown }>
  registerWorkerMetadata: () => void
  sendMessage: (...args: unknown[]) => void
}

let engine: MockEngine
let sdk: Internal

function makeSdk(autoOpen = true) {
  engine = new MockEngine({ autoOpen })
  engine.install()
  sdk = registerWorker('ws://test:49135', {
    invocationTimeoutMs: 60_000,
    reconnectionConfig: { maxRetries: 0 },
  }) as Internal
  return engine.socket
}

function payloadForEnvelopeBytes(targetBytes: number): string {
  const emptyEnvelopeBytes = new TextEncoder().encode(JSON.stringify(message(''))).byteLength
  return 'x'.repeat(targetBytes - emptyEnvelopeBytes)
}

function expectPayloadTooLarge(error: unknown, size: number) {
  expect(error).toMatchObject({
    name: 'InvocationError',
    code: 'payload_too_large',
    message: `payload_too_large: Serialized JSON envelope is ${size} bytes; limit is ${LIMIT} bytes. Use channels for larger payloads: ${LINK}`,
  })
}

afterEach(async () => {
  if (sdk) await sdk.shutdown()
  engine?.uninstall()
  vi.restoreAllMocks()
})

describe('Browser JSON frame size', () => {
  it.each([-1, 0, 1])('enforces the inclusive complete-envelope boundary (%i)', (delta) => {
    makeSdk()
    const envelope = message(payloadForEnvelopeBytes(LIMIT + delta))
    if (delta > 0) {
      const boundaryError = (() => {
        try {
          sdk.prepareJson(envelope)
        } catch (error) {
          return error
        }
        throw new Error('Expected oversized boundary to be rejected')
      })()
      expectPayloadTooLarge(boundaryError, LIMIT + delta)
    } else {
      expect(new TextEncoder().encode(sdk.prepareJson(envelope)).byteLength).toBe(LIMIT + delta)
    }
  })

  it('counts UTF-8 bytes and JSON escapes, not UTF-16 characters', () => {
    makeSdk()
    const envelope = message('😀é\n"\\'.repeat(1_400_000))
    const serialized = JSON.stringify(envelope)
    expect(envelope.data.length).toBeLessThan(LIMIT)
    expect(new TextEncoder().encode(serialized).byteLength).toBeGreaterThan(LIMIT)
    expect(() => sdk.prepareJson(envelope)).toThrow('limit is 16777216 bytes')
  })

  it('rejects oversized local invocation arguments, clears pending state and preserves function id', async () => {
    makeSdk()
    const payload = payloadForEnvelopeBytes(LIMIT + 1)
    const promise = sdk.call({ function_id: 'browser::large', payload })
    const rejection = await promise.catch((error) => error)
    expect(rejection).toMatchObject({
      name: 'InvocationError',
      code: 'payload_too_large',
      function_id: 'browser::large',
    })
    const serializedBytes = new TextEncoder().encode(
      JSON.stringify({
        type: MessageType.InvokeFunction,
        invocation_id: '00000000-0000-4000-8000-000000000000',
        function_id: 'browser::large',
        data: payload,
      }),
    ).byteLength
    expect((rejection as Error).message).toBe(
      `payload_too_large: Serialized JSON envelope is ${serializedBytes} bytes; limit is ${LIMIT} bytes. Use channels for larger payloads: ${LINK}`,
    )
    expect(sdk.invocations.size).toBe(0)
    expect(sdk.messagesToSend).toHaveLength(0)
    expect(engine.findAllSent('invokefunction').some((entry) => entry.function_id === 'browser::large')).toBe(false)
    expect(engine.socket.readyState).toBe(WebSocket.OPEN)

    await sdk.call({
      function_id: 'browser::small',
      payload: { ok: true },
      action: { type: 'void' },
    })
    const small = engine.findAllSent('invokefunction').find((entry) => entry.function_id === 'browser::small')
    expect(small).toBeDefined()
    expect(engine.socket.readyState).toBe(WebSocket.OPEN)
  })

  it('replaces an oversized invocation result with its correlated error fallback and keeps the socket usable', async () => {
    const socket = makeSdk()
    await engine.waitForOpen()
    sdk.registerFunction('browser::handler', async () => payloadForEnvelopeBytes(LIMIT + 1))
    engine.invokeFunction('browser::handler', {}, 'invoke-result-1')
    await new Promise<void>((resolve) => setTimeout(resolve, 0))
    const result = socket.findAllSent('invocationresult').find((entry) => entry.invocation_id === 'invoke-result-1')
    expect(result).toMatchObject({
      invocation_id: 'invoke-result-1',
      function_id: 'browser::handler',
      error: { code: 'payload_too_large' },
    })
    expect(result).not.toHaveProperty('result')
    // The wire body carries the bare message; the code travels in `code`, not as a message prefix.
    expect((result?.error as { message: string }).message).toMatch(
      /^Serialized JSON envelope is \d+ bytes; limit is 16777216 bytes\. Use channels for larger payloads: https:\/\/iii\.dev\/docs\/creating-workers\/channels#payload-size$/,
    )
    expect(socket.readyState).toBe(WebSocket.OPEN)
    await sdk.call({ function_id: 'browser::still-open', payload: {}, action: { type: 'void' } })
    expect(socket.findAllSent('invokefunction').some((entry) => entry.function_id === 'browser::still-open')).toBe(true)
  })

  it('does not retain oversized registrations and snapshots accepted registration JSON independently', async () => {
    const socket = makeSdk()
    await engine.waitForOpen()
    expect(() =>
      sdk.registerFunction('browser::rejected', async () => null, {
        metadata: { note: 'x'.repeat(LIMIT) },
      }),
    ).toThrow('limit is 16777216 bytes')
    expect(sdk.functions.has('browser::rejected')).toBe(false)

    const metadata = { nested: { value: 'original' } }
    const argsFormat = { type: 'object' as const, properties: { nested: { type: 'string' } } }
    sdk.registerFunction('browser::accepted', async () => null, { metadata, args_format: argsFormat })
    metadata.nested.value = 'mutated'
    argsFormat.properties.nested.type = 'mutated'

    const replaySocket = new WebSocket('ws://test:49135') as unknown as MockWebSocket
    replaySocket.readyState = WebSocket.OPEN
    sdk.ws = replaySocket as unknown as WebSocket
    sdk.onSocketOpen()

    // A function with `args_format` waits for this connection's `WorkerRegistered` (observe.v2).
    replaySocket.simulateMessage(
      JSON.stringify({ type: 'workerregistered', worker_id: 'w-1', features: ['observe.v1', 'observe.v2'] }),
    )

    const replayFunction = replaySocket
      .findAllSent('registerfunction')
      .find((entry) => entry.id === 'browser::accepted')
    expect(replayFunction?.metadata).toEqual({ nested: { value: 'original' } })
    expect(replayFunction?.args_format).toEqual({ type: 'object', properties: { nested: { type: 'string' } } })
    expect(socket.readyState).toBe(WebSocket.OPEN)
  })

  it('contains permanent registration rejection during real onopen replay', () => {
    const socket = makeSdk(false)
    sdk.registerFunction('browser::first', async () => null)
    const retainedFunction = sdk.functions.get('browser::first')
    if (retainedFunction) retainedFunction.message.metadata = { note: 'x'.repeat(LIMIT) }
    sdk.registerFunction('browser::later', async () => null)
    sdk.messagesToSend.push({ type: 'pong' })

    expect(() => socket.simulateOpen()).not.toThrow()
    expect(socket.findAllSent('registerfunction').some((entry) => entry.id === 'browser::later')).toBe(true)
    expect(socket.sentMessages[socket.sentMessages.length - 1]).toBe('{"type":"pong"}')
    expect(sdk.messagesToSend).toHaveLength(0)
    expect(socket.readyState).toBe(WebSocket.OPEN)
  })

  it('guards raw sends and reattach, while the same socket sends a later small message', () => {
    const socket = makeSdk(false)
    socket.simulateOpen()
    socket.sentRaw.length = 0
    sdk.workerId = 'worker-1'
    sdk.reattachToken = 'token'
    const oversizedReattach = JSON.stringify({
      type: MessageType.Reattach,
      previous_worker_id: 'worker-1',
      reattach_token: 'x'.repeat(LIMIT),
    })
    expect(() => sdk.sendMessageRaw(oversizedReattach)).toThrow('limit is 16777216 bytes')
    expect(socket.readyState).toBe(WebSocket.OPEN)
    expect(() => sdk.sendMessageRaw(JSON.stringify({ type: 'pong' }))).not.toThrow()
    expect(socket.sentMessages).toContain('{"type":"pong"}')

    sdk.registerWorkerMetadata = () => undefined
    sdk.reattachToken = 'x'.repeat(LIMIT)
    expect(() => sdk.onSocketOpen()).not.toThrow()
    expect(socket.readyState).toBe(WebSocket.OPEN)
    expect(() => sdk.sendMessageRaw(JSON.stringify({ type: 'ping' }))).not.toThrow()
  })

  it('contains an oversized queued result as a correlated fallback during replay', () => {
    const socket = makeSdk(false)
    const oversizedResult = {
      type: MessageType.InvocationResult,
      invocation_id: 'queued-result',
      function_id: 'browser::handler',
      result: payloadForEnvelopeBytes(LIMIT + 1),
    }
    sdk.messagesToSend.push(oversizedResult)
    expect(() => socket.simulateOpen()).not.toThrow()
    const fallback = socket.findAllSent('invocationresult').find((entry) => entry.invocation_id === 'queued-result')
    expect(fallback).toMatchObject({ error: { code: 'payload_too_large' } })
    expect(fallback).not.toHaveProperty('result')
    expect(socket.readyState).toBe(WebSocket.OPEN)
  })
})
