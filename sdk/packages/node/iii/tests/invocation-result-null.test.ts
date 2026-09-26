import { afterEach, describe, expect, it, vi } from 'vitest'
import { registerWorker } from '../src/iii'
import { MessageType } from '../src/iii-types'

/**
 * `null` and "no result" are different answers (MOT-4732). `state::get` on a
 * missing key is `null`; the engine forwards it as `"result": null` and the
 * SDK must resolve `null` so the natural `=== null` check fires. A handler
 * that returned nothing omits the field, and that still resolves `undefined`.
 */
type InternalSdk = {
  trigger: (request: Record<string, unknown>) => Promise<unknown>
  onMessage: (data: string) => void
  sendMessage: ReturnType<typeof vi.fn>
  shutdown: () => Promise<void>
}

function makeSdk(): InternalSdk {
  return registerWorker('ws://127.0.0.1:0', {
    enableMetricsReporting: false,
    reconnectionConfig: { maxRetries: 0 },
  }) as unknown as InternalSdk
}

function answer(sdk: InternalSdk, pending: Promise<unknown>, reply: Record<string, unknown>) {
  const invoke = sdk.sendMessage.mock.calls.find(([type]) => type === MessageType.InvokeFunction)
  expect(invoke).toBeDefined()
  const { invocation_id, function_id } = invoke?.[1] as { invocation_id: string; function_id: string }
  sdk.onMessage(
    JSON.stringify({ type: MessageType.InvocationResult, invocation_id, function_id, ...reply }),
  )
  return pending
}

describe('invocation result: null vs absent', () => {
  let sdk: InternalSdk

  afterEach(async () => {
    await sdk.shutdown()
  })

  it('resolves null when the frame carries "result": null', async () => {
    sdk = makeSdk()
    sdk.sendMessage = vi.fn()

    const pending = sdk.trigger({ function_id: 'state::get', payload: { scope: 's', key: 'missing' } })

    await expect(answer(sdk, pending, { result: null })).resolves.toBeNull()
  })

  it('resolves undefined when the frame omits result', async () => {
    sdk = makeSdk()
    sdk.sendMessage = vi.fn()

    const pending = sdk.trigger({ function_id: 'side::effect', payload: {} })

    await expect(answer(sdk, pending, {})).resolves.toBeUndefined()
  })

  it('sends "result": null when a handler returns null, and omits it for undefined', async () => {
    sdk = makeSdk()
    sdk.sendMessage = vi.fn()
    const internal = sdk as unknown as {
      registerFunction: (id: string, handler: () => Promise<unknown>) => unknown
      onInvokeFunction: (invocation_id: string, function_id: string, input: unknown) => Promise<unknown>
    }
    internal.registerFunction('kv::get', async () => null)
    internal.registerFunction('kv::touch', async () => undefined)

    await internal.onInvokeFunction('inv-null', 'kv::get', {})
    await internal.onInvokeFunction('inv-void', 'kv::touch', {})

    const replies = sdk.sendMessage.mock.calls
      .filter(([type]) => type === MessageType.InvocationResult)
      .map(([, message]) => message as Record<string, unknown>)
    const forNull = replies.find((m) => m.invocation_id === 'inv-null')
    const forVoid = replies.find((m) => m.invocation_id === 'inv-void')
    expect(forNull).toMatchObject({ result: null })
    expect(JSON.stringify(forNull)).toContain('"result":null')
    expect(JSON.stringify(forVoid)).not.toContain('result')
  })
})
