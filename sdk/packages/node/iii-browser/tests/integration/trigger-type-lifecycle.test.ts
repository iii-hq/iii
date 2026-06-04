import { afterEach, describe, expect, it, vi } from 'vitest'
import { WebSocket } from 'ws'

globalThis.WebSocket = WebSocket as unknown as typeof globalThis.WebSocket

import { TriggerAction, registerWorker } from '../../src/index'
import type { ISdk } from '../../src/types'
import type { TriggerConfig } from '../../src/triggers'
import { engineWsUrl, sleep } from './utils'

const TRIGGER_TYPE_ID = 'test.tt-lifecycle.browser'
const CONSUMER_FN = 'test.tt-lifecycle.browser.consumer'
const FIRE_FN = 'test.tt-lifecycle.browser.fire'
const TRIGGER_CONFIG = { tag: 'test' }

type TestTriggerConfig = { tag: string }

describe('Trigger type lifecycle (two workers)', () => {
  let provider: ISdk
  let consumer: ISdk
  const bindings = new Map<string, TriggerConfig<TestTriggerConfig>>()
  let registerTriggerSpy: ReturnType<typeof vi.fn>
  let unregisterTriggerSpy: ReturnType<typeof vi.fn>
  let handlerSpy: ReturnType<typeof vi.fn>

  function createProvider(): ISdk {
    bindings.clear()
    registerTriggerSpy = vi.fn(async (cfg: TriggerConfig<TestTriggerConfig>) => {
      bindings.set(cfg.id, cfg)
    })
    unregisterTriggerSpy = vi.fn()

    const sdk = registerWorker(engineWsUrl, {
      reconnectionConfig: { maxRetries: 3, initialDelayMs: 100, maxDelayMs: 1000 },
    })

    sdk.registerTriggerType(
      { id: TRIGGER_TYPE_ID, description: 'Browser SDK lifecycle test trigger type' },
      {
        registerTrigger: registerTriggerSpy,
        unregisterTrigger: async (cfg) => {
          const stored = bindings.get(cfg.id)
          bindings.delete(cfg.id)
          await unregisterTriggerSpy(stored ?? cfg)
        },
      },
    )

    sdk.registerFunction(FIRE_FN, async (payload: { n?: number }) => {
      for (const binding of bindings.values()) {
        await sdk.trigger({
          function_id: binding.function_id,
          payload,
          action: TriggerAction.Void(),
        })
      }
      return { fired: bindings.size }
    })

    return sdk
  }

  async function createConsumer(): Promise<void> {
    handlerSpy = vi.fn(async (payload: { n?: number }) => ({ ok: true, payload }))
    consumer = registerWorker(engineWsUrl, {
      reconnectionConfig: { maxRetries: 3, initialDelayMs: 100, maxDelayMs: 1000 },
    })
    consumer.registerFunction(CONSUMER_FN, handlerSpy)
    consumer.registerTrigger({
      type: TRIGGER_TYPE_ID,
      function_id: CONSUMER_FN,
      config: TRIGGER_CONFIG,
    })
    await sleep(400)
  }

  afterEach(async () => {
    await provider?.shutdown()
    await consumer?.shutdown()
  })

  it('fires all bound functions when the trigger type is fired', async () => {
    provider = createProvider()
    await sleep(300)
    await createConsumer()

    expect(registerTriggerSpy).toHaveBeenCalledTimes(1)
    expect(registerTriggerSpy.mock.calls[0][0].function_id).toBe(CONSUMER_FN)

    await provider.trigger<{ n: number }, { fired: number }>({
      function_id: FIRE_FN,
      payload: { n: 1 },
    })
    await sleep(400)

    expect(handlerSpy).toHaveBeenCalledTimes(1)
    expect(handlerSpy.mock.calls[0][0]).toMatchObject({ n: 1 })
  })

  it('re-binds triggers when the provider worker reconnects', async () => {
    provider = createProvider()
    await sleep(300)
    await createConsumer()

    const boundTriggerId = registerTriggerSpy.mock.calls[0][0].id as string
    registerTriggerSpy.mockClear()

    await provider.shutdown()
    await sleep(400)

    provider = createProvider()
    await sleep(400)

    expect(registerTriggerSpy).toHaveBeenCalledTimes(1)
    expect(registerTriggerSpy.mock.calls[0][0].id).toBe(boundTriggerId)
    expect(registerTriggerSpy.mock.calls[0][0].function_id).toBe(CONSUMER_FN)

    handlerSpy.mockClear()
    await provider.trigger({
      function_id: FIRE_FN,
      payload: { n: 2 },
    })
    await sleep(400)

    expect(handlerSpy).toHaveBeenCalledTimes(1)
    expect(handlerSpy.mock.calls[0][0]).toMatchObject({ n: 2 })
  })

  it('invokes unregisterTrigger on the provider when the consumer disconnects', async () => {
    provider = createProvider()
    await sleep(300)
    await createConsumer()

    unregisterTriggerSpy.mockClear()

    await consumer.shutdown()
    consumer = undefined as unknown as ISdk
    await sleep(400)

    expect(unregisterTriggerSpy).toHaveBeenCalledTimes(1)
    expect(unregisterTriggerSpy.mock.calls[0][0]).toMatchObject({
      function_id: CONSUMER_FN,
      config: TRIGGER_CONFIG,
    })
  })
})
