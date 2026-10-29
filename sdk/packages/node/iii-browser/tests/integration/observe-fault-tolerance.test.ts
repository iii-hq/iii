// Derived from sdk/packages/node/iii/tests/observe-fault-tolerance.test.ts by scripts/sync-browser-observe.py; edit those, not this file.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { registerWorker } from '../../src/index'
import type { Observed, Observer } from '../../src/observe-types'
import type { ISdk } from '../../src/types'
import { engineWsUrl, sleep } from './utils'

/**
 * Fault tolerance to bad worker sequencing: an observer of a function that is
 * not (yet) hosted stays pending and becomes active when a host registers the
 * function, runs its `onObserverAdded` and acks it, including when the host
 * restarts or a standby takes over (v2 §7).
 *
 * The suite shares one long-running engine, so every id carries a per-run
 * suffix: an observer left behind by a crashed previous run must never reach
 * this run's spies.
 */
const RUN = crypto.randomUUID().slice(0, 8)
const ARGS = { tag: 'fault-tolerance' }

type HostHarness = {
  sdk: ISdk
  addedSpy: ReturnType<typeof vi.fn>
  fire: (payload: Record<string, unknown>) => Promise<unknown>
}

describe('Observer fault tolerance (pending until hosted)', () => {
  const clients: ISdk[] = []

  function createHost(sourceId: string, opts: { standby?: boolean; name?: string } = {}): HostHarness {
    const addedSpy = vi.fn((_obs: Observer) => undefined)
    // Named off the per-test source id: unique against every other live
    // worker, yet stable across this test's host restart.
    const sdk = registerWorker(engineWsUrl, {
      workerName: opts.name ?? `${sourceId}::host`,
      reconnectionConfig: { maxRetries: 3, initialDelayMs: 100, maxDelayMs: 1000 },
    })
    clients.push(sdk)
    sdk
      .registerFunction(sourceId, (event: Record<string, unknown>) => event, {
        args_format: { type: 'object', required: ['tag'], properties: { tag: { type: 'string' } } },
        private: true,
        standby: opts.standby,
      })
      .onObserverAdded(addedSpy)
    return { sdk, addedSpy, fire: (payload) => sdk.call({ function_id: sourceId, payload }) }
  }

  function createConsumer(sourceId: string, consumerFnId: string) {
    const handlerSpy = vi.fn(async (envelope: Observed) => ({ ok: true, envelope }))
    const sdk = registerWorker(engineWsUrl, {
      workerName: `${sourceId}::consumer`,
      reconnectionConfig: { maxRetries: 3, initialDelayMs: 100, maxDelayMs: 1000 },
    })
    clients.push(sdk)
    sdk.registerFunction(consumerFnId, handlerSpy)
    const handle = sdk.observe({ function_id: sourceId, args: ARGS, capture: ['input'] }, consumerFnId)
    return { sdk, handle, handlerSpy }
  }

  afterEach(async () => {
    while (clients.length > 0) {
      await clients.pop()?.shutdown()
    }
    vi.restoreAllMocks()
  })

  it('an observer created before its function is hosted stays pending, then becomes active when the host arrives', async () => {
    const SOURCE = `test::deferred-browser-${RUN}`
    const { handle, handlerSpy } = createConsumer(SOURCE, `${SOURCE}::consumer`)
    await sleep(400)
    expect(handle.state).toBe('pending')

    const host = createHost(SOURCE)
    await handle.ready()
    expect(host.addedSpy).toHaveBeenCalledTimes(1)
    expect(host.addedSpy.mock.calls[0][0]).toMatchObject({ id: handle.id, args: ARGS })

    await host.fire({ n: 1 })
    await sleep(400)
    expect(handlerSpy).toHaveBeenCalledTimes(1)
    expect(handlerSpy.mock.calls[0][0]).toMatchObject({ observer_id: handle.id, input: { n: 1 } })
  })

  it('a host that stops and starts again runs its hooks for the observers it gets back', async () => {
    const SOURCE = `test::host-restart-browser-${RUN}`
    let host = createHost(SOURCE)
    await sleep(300)
    const { handle, handlerSpy } = createConsumer(SOURCE, `${SOURCE}::consumer`)
    await handle.ready()
    expect(host.addedSpy).toHaveBeenCalledTimes(1)

    await host.sdk.shutdown()
    clients.splice(clients.indexOf(host.sdk), 1)
    await sleep(400)
    expect(handle.state).not.toBe('active')

    host = createHost(SOURCE)
    await handle.ready()
    expect(host.addedSpy).toHaveBeenCalledTimes(1)
    expect(host.addedSpy.mock.calls[0][0]).toMatchObject({ id: handle.id, args: ARGS })

    await host.fire({ n: 2 })
    await sleep(400)
    expect(handlerSpy).toHaveBeenCalledTimes(1)
    expect(handlerSpy.mock.calls[0][0]).toMatchObject({ input: { n: 2 } })
  })

  it('a standby takes the function over when its host leaves, and runs its hooks for every observer', async () => {
    const SOURCE = `test::standby-browser-${RUN}`
    const host = createHost(SOURCE, { name: `${SOURCE}::host-a` })
    await sleep(300)
    const standby = createHost(SOURCE, { name: `${SOURCE}::host-b`, standby: true })
    const { handle, handlerSpy } = createConsumer(SOURCE, `${SOURCE}::consumer`)
    await handle.ready()
    await sleep(1500) // the standby retries meanwhile
    expect(host.addedSpy).toHaveBeenCalledTimes(1)
    expect(standby.addedSpy).not.toHaveBeenCalled()

    await host.sdk.shutdown()
    clients.splice(clients.indexOf(host.sdk), 1)
    await sleep(2500)
    expect(standby.addedSpy).toHaveBeenCalledTimes(1)
    await handle.ready()

    await standby.fire({ n: 3 })
    await sleep(400)
    expect(handlerSpy).toHaveBeenCalledTimes(1)
    expect(handlerSpy.mock.calls[0][0]).toMatchObject({ input: { n: 3 } })
  })
})
