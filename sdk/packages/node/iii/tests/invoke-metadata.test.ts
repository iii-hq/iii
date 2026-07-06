import { afterEach, describe, expect, it, vi } from 'vitest'
import { registerWorker } from '../src/iii'
import { MessageType } from '../src/iii-types'
import type { RemoteFunctionHandler } from '../src/types'

/**
 * Internal surface of the SDK used by these unit tests. The public
 * {@link IIIClient} type does not expose the private dispatch helpers, so we
 * cast through this shape (mirroring the approach in
 * register-worker-metadata.test.ts) to drive the inbound/outbound paths
 * directly without standing up a live engine.
 */
type InternalSdk = {
  registerFunction: (
    id: string,
    handler: RemoteFunctionHandler,
    options?: Record<string, unknown>,
  ) => { id: string; unregister: () => void }
  trigger: (request: Record<string, unknown>) => Promise<unknown>
  onInvokeFunction: (
    invocation_id: string | undefined,
    function_id: string,
    input: unknown,
    metadata?: unknown,
    traceparent?: string,
    baggage?: string,
  ) => Promise<unknown>
  onMessage: (data: string) => void
  sendMessage: ReturnType<typeof vi.fn>
  toWireFormat: (type: MessageType, message: Record<string, unknown>) => Record<string, unknown>
  shutdown: () => Promise<void>
}

function makeSdk(): InternalSdk {
  return registerWorker('ws://127.0.0.1:0', {
    enableMetricsReporting: false,
    reconnectionConfig: { maxRetries: 0 },
  }) as unknown as InternalSdk
}

describe('invocation metadata — inbound (handler receives metadata)', () => {
  let sdk: InternalSdk

  afterEach(async () => {
    await sdk.shutdown()
  })

  it('passes metadata to the handler as a separate argument when present', async () => {
    sdk = makeSdk()
    let received: { data: unknown; metadata: unknown } | undefined

    sdk.registerFunction('fn.meta.present', async (data, metadata) => {
      received = { data, metadata }
      return { ok: true }
    })

    await sdk.onInvokeFunction(undefined, 'fn.meta.present', { a: 1 }, { tenant: 't1', attempt: 2 })

    expect(received).toEqual({ data: { a: 1 }, metadata: { tenant: 't1', attempt: 2 } })
  })

  it('passes undefined metadata to the handler when absent', async () => {
    sdk = makeSdk()
    let received: { data: unknown; metadata: unknown } | undefined

    sdk.registerFunction('fn.meta.absent', async (data, metadata) => {
      received = { data, metadata }
      return { ok: true }
    })

    await sdk.onInvokeFunction(undefined, 'fn.meta.absent', { a: 1 })

    expect(received?.data).toEqual({ a: 1 })
    expect(received?.metadata).toBeUndefined()
  })

  it('keeps existing single-argument handlers working (extra arg ignored)', async () => {
    sdk = makeSdk()
    let seen: unknown

    // Handler declares only `data`; metadata is still supplied on the wire.
    sdk.registerFunction('fn.meta.legacy', async (data: { a: number }) => {
      seen = data
      return { ok: true }
    })

    await expect(
      sdk.onInvokeFunction(undefined, 'fn.meta.legacy', { a: 9 }, { ignored: true }),
    ).resolves.toBeUndefined()
    expect(seen).toEqual({ a: 9 })
  })

  it('threads metadata from a raw inbound InvokeFunction message to the handler', async () => {
    sdk = makeSdk()
    let resolveReceived: (value: { data: unknown; metadata: unknown }) => void
    const received = new Promise<{ data: unknown; metadata: unknown }>((resolve) => {
      resolveReceived = resolve
    })

    sdk.registerFunction('fn.meta.inbound', async (data, metadata) => {
      resolveReceived({ data, metadata })
      return {}
    })

    sdk.onMessage(
      JSON.stringify({
        type: MessageType.InvokeFunction,
        function_id: 'fn.meta.inbound',
        data: { hello: 'world' },
        metadata: { tenant: 't9' },
      }),
    )

    await expect(received).resolves.toEqual({
      data: { hello: 'world' },
      metadata: { tenant: 't9' },
    })
  })

  it('parses an inbound InvokeFunction message without metadata (backward compatible)', async () => {
    sdk = makeSdk()
    let resolveReceived: (value: { data: unknown; metadata: unknown }) => void
    const received = new Promise<{ data: unknown; metadata: unknown }>((resolve) => {
      resolveReceived = resolve
    })

    sdk.registerFunction('fn.meta.inbound.none', async (data, metadata) => {
      resolveReceived({ data, metadata })
      return {}
    })

    // Mirrors a message from an older engine that never sends `metadata`.
    sdk.onMessage(
      JSON.stringify({
        type: MessageType.InvokeFunction,
        function_id: 'fn.meta.inbound.none',
        data: { hello: 'world' },
      }),
    )

    const result = await received
    expect(result.data).toEqual({ hello: 'world' })
    expect(result.metadata).toBeUndefined()
  })
})

describe('invocation metadata — outbound (trigger serializes metadata)', () => {
  let sdk: InternalSdk

  afterEach(async () => {
    await sdk.shutdown()
  })

  it('includes metadata on the outbound InvokeFunction message (sync/await path)', () => {
    sdk = makeSdk()
    sdk.sendMessage = vi.fn()

    // Non-void trigger returns a never-resolving promise here (no engine to
    // respond); we only assert the synchronous send. Swallow the eventual
    // shutdown rejection so it does not surface as an unhandled rejection.
    void sdk
      .trigger({ function_id: 'fn', payload: { x: 1 }, metadata: { tenant: 't1' } })
      .catch(() => {})

    const invoke = sdk.sendMessage.mock.calls.find(([type]) => type === MessageType.InvokeFunction)
    expect(invoke).toBeDefined()
    expect(invoke?.[1].metadata).toEqual({ tenant: 't1' })
  })

  it('includes metadata on the outbound InvokeFunction message (void path)', async () => {
    sdk = makeSdk()
    sdk.sendMessage = vi.fn()

    await sdk.trigger({
      function_id: 'fn',
      payload: { x: 1 },
      action: { type: 'void' },
      metadata: { tenant: 't2' },
    })

    const invoke = sdk.sendMessage.mock.calls.find(([type]) => type === MessageType.InvokeFunction)
    expect(invoke).toBeDefined()
    expect(invoke?.[1].metadata).toEqual({ tenant: 't2' })
  })

  it('omits metadata from the message object when not provided', async () => {
    sdk = makeSdk()
    sdk.sendMessage = vi.fn()

    await sdk.trigger({ function_id: 'fn', payload: { x: 1 }, action: { type: 'void' } })

    const invoke = sdk.sendMessage.mock.calls.find(([type]) => type === MessageType.InvokeFunction)
    expect(invoke).toBeDefined()
    expect(invoke?.[1].metadata).toBeUndefined()
    // `JSON.stringify` drops undefined-valued keys, so metadata never hits the wire.
    expect(JSON.stringify(invoke?.[1])).not.toContain('metadata')
  })
})

describe('invocation metadata — protocol (de)serialization round-trip', () => {
  let sdk: InternalSdk

  afterEach(async () => {
    await sdk.shutdown()
  })

  it('round-trips metadata through wire serialization', () => {
    sdk = makeSdk()

    const wire = sdk.toWireFormat(MessageType.InvokeFunction, {
      function_id: 'fn',
      data: { x: 1 },
      metadata: { tenant: 't1', nested: { k: [1, 2, 3] } },
    })

    const parsed = JSON.parse(JSON.stringify(wire))
    expect(parsed.type).toBe(MessageType.InvokeFunction)
    expect(parsed.metadata).toEqual({ tenant: 't1', nested: { k: [1, 2, 3] } })
  })

  it('omits metadata from serialized JSON when absent', () => {
    sdk = makeSdk()

    const wire = sdk.toWireFormat(MessageType.InvokeFunction, {
      function_id: 'fn',
      data: { x: 1 },
    })

    expect(JSON.stringify(wire)).not.toContain('metadata')
  })
})
