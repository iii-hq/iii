import { afterEach, describe, expect, it, vi } from 'vitest'
import { TriggerAction, registerWorker } from '../src/index'
import type { IIIClient } from '../src/types'
import type { TriggerConfig } from '../src/triggers'
import { engineWsUrl, sleep } from './utils'

/**
 * Fault tolerance to bad worker sequencing: the engine parks trigger
 * registrations whose trigger type is not (yet) available and activates them
 * when the type's provider shows up — including when the provider restarts.
 *
 * The suite shares one long-running engine, so every id carries a per-run
 * suffix: a parked intent left behind by a crashed previous run must never be
 * re-activated into this run's spies.
 */
const RUN = crypto.randomUUID().slice(0, 8)
const TRIGGER_CONFIG = { tag: 'fault-tolerance' }

type TestTriggerConfig = { tag: string }

type ProviderHarness = {
  sdk: IIIClient
  bindings: Map<string, TriggerConfig<TestTriggerConfig>>
  registerTriggerSpy: ReturnType<typeof vi.fn>
  fire: (payload: Record<string, unknown>) => Promise<unknown>
}

describe('Trigger fault tolerance (deferred registration)', () => {
  const clients: IIIClient[] = []

  function createProvider(triggerTypeId: string, fireFnId: string): ProviderHarness {
    const bindings = new Map<string, TriggerConfig<TestTriggerConfig>>()
    const registerTriggerSpy = vi.fn(async (cfg: TriggerConfig<TestTriggerConfig>) => {
      bindings.set(cfg.id, cfg)
    })

    // Named off the per-test trigger type id: unique against every other live
    // worker, yet stable across this test's provider restart — the restarted
    // provider is meant to be the same identity coming back.
    const sdk = registerWorker(engineWsUrl, {
      workerName: `${triggerTypeId}::provider`,
      reconnectionConfig: { maxRetries: 3, initialDelayMs: 100, maxDelayMs: 1000 },
    })
    clients.push(sdk)

    sdk.registerTriggerType(
      { id: triggerTypeId, description: 'Node SDK fault-tolerance test trigger type' },
      {
        registerTrigger: registerTriggerSpy,
        unregisterTrigger: async (cfg) => {
          bindings.delete(cfg.id)
        },
      },
    )

    sdk.registerFunction(fireFnId, async (payload: Record<string, unknown>) => {
      for (const binding of bindings.values()) {
        await sdk.trigger({
          function_id: binding.function_id,
          payload,
          action: TriggerAction.Void(),
        })
      }
      return { fired: bindings.size }
    })

    return {
      sdk,
      bindings,
      registerTriggerSpy,
      fire: (payload) => sdk.trigger({ function_id: fireFnId, payload }),
    }
  }

  function createConsumer(
    triggerTypeId: string,
    consumerFnId: string,
  ): { sdk: IIIClient; handlerSpy: ReturnType<typeof vi.fn> } {
    const handlerSpy = vi.fn(async (payload: Record<string, unknown>) => ({ ok: true, payload }))
    const sdk = registerWorker(engineWsUrl, {
      workerName: `${triggerTypeId}::consumer`,
      reconnectionConfig: { maxRetries: 3, initialDelayMs: 100, maxDelayMs: 1000 },
    })
    clients.push(sdk)

    sdk.registerFunction(consumerFnId, handlerSpy)
    sdk.registerTrigger({
      type: triggerTypeId,
      function_id: consumerFnId,
      config: TRIGGER_CONFIG,
    })

    return { sdk, handlerSpy }
  }

  afterEach(async () => {
    while (clients.length > 0) {
      await clients.pop()?.shutdown()
    }
    vi.restoreAllMocks()
  })

  it('parks a trigger registered before its type exists and activates it when the provider arrives', async () => {
    const TRIGGER_TYPE_ID = `test::deferred-node-${RUN}`
    const CONSUMER_FN = `test::deferred-node-${RUN}::consumer`
    const FIRE_FN = `test::deferred-node-${RUN}::fire`

    // The engine must not error-ack a deferred registration — the SDK would
    // log it as "[iii] Trigger registration failed".
    const consoleErrorSpy = vi.spyOn(console, 'error')

    // Bad sequencing: the consumer binds to a trigger type whose provider
    // has not connected yet.
    const { handlerSpy } = createConsumer(TRIGGER_TYPE_ID, CONSUMER_FN)
    await sleep(400)

    const registrationFailures = consoleErrorSpy.mock.calls
      .map((args) => args.join(' '))
      .filter((msg) => msg.includes('Trigger registration failed') && msg.includes(TRIGGER_TYPE_ID))
    expect(registrationFailures).toEqual([])

    // The provider shows up late and registers the trigger type: the parked
    // registration must be delivered to it.
    const provider = createProvider(TRIGGER_TYPE_ID, FIRE_FN)
    await sleep(500)

    expect(provider.registerTriggerSpy).toHaveBeenCalledTimes(1)
    expect(provider.registerTriggerSpy.mock.calls[0][0]).toMatchObject({
      function_id: CONSUMER_FN,
      config: TRIGGER_CONFIG,
    })

    // End to end: firing the recovered binding invokes the consumer.
    await provider.fire({ n: 1 })
    await sleep(400)

    expect(handlerSpy).toHaveBeenCalledTimes(1)
    expect(handlerSpy.mock.calls[0][0]).toMatchObject({ n: 1 })
  })

  it('re-registers another worker’s trigger after the provider stops and starts again', async () => {
    const TRIGGER_TYPE_ID = `test::provider-restart-node-${RUN}`
    const CONSUMER_FN = `test::provider-restart-node-${RUN}::consumer`
    const FIRE_FN = `test::provider-restart-node-${RUN}::fire`

    let provider = createProvider(TRIGGER_TYPE_ID, FIRE_FN)
    await sleep(300)

    const { handlerSpy } = createConsumer(TRIGGER_TYPE_ID, CONSUMER_FN)
    await sleep(400)

    expect(provider.registerTriggerSpy).toHaveBeenCalledTimes(1)
    const boundTriggerId = provider.registerTriggerSpy.mock.calls[0][0].id as string

    // Stop the provider: its trigger type is unregistered and the consumer's
    // binding is disabled (parked) engine-side.
    await provider.sdk.shutdown()
    clients.splice(clients.indexOf(provider.sdk), 1)
    await sleep(400)

    // Start the provider again: the engine re-delivers the parked binding to
    // the fresh registrator.
    provider = createProvider(TRIGGER_TYPE_ID, FIRE_FN)
    await sleep(500)

    expect(provider.registerTriggerSpy).toHaveBeenCalledTimes(1)
    expect(provider.registerTriggerSpy.mock.calls[0][0]).toMatchObject({
      id: boundTriggerId,
      function_id: CONSUMER_FN,
      config: TRIGGER_CONFIG,
    })

    // The recovered binding still fires the consumer's function.
    await provider.fire({ n: 2 })
    await sleep(400)

    expect(handlerSpy).toHaveBeenCalledTimes(1)
    expect(handlerSpy.mock.calls[0][0]).toMatchObject({ n: 2 })
  })
})
