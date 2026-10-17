import { afterEach, describe, expect, it } from 'vitest'
import { ObserveRejection } from '../src/index'
import type { Observer } from '../src/observe-types'
import {
  type Activation,
  closeWorkers,
  collector,
  dropConnection,
  frameCounter,
  localDispatchReady,
  sleep,
  stable,
  waitFor,
  worker,
} from './observe-e2e-utils'

/**
 * Observable functions against the real engine (README §7 phase 2): local
 * dispatch, the reporter, the owner hooks and owner delivery (sdk.md §7).
 */
afterEach(closeWorkers)

// Function ids of this run: observers of an earlier run's workers outlive them (suspended, 30 s grace).
const RUN = crypto.randomUUID().slice(0, 8)

const ALL = ['input', 'output', 'error', 'caller', 'metadata'] as const

/** Waits until `got` holds `n` activations, then a little longer to catch extra ones. */
async function exactly(got: Activation[], n: number, what: string, timeoutMs = 5000) {
  await waitFor(() => got.length >= n, what, timeoutMs)
  await sleep(300)
  expect(got).toHaveLength(n)
}

describe('equivalence: local and routed calls look the same to observers', () => {
  it("dispatch: 'auto' (local) and 'remote' give identical envelopes apart from ids and timings", async () => {
    const owner = worker('eq-owner')
    const watcher = worker('eq-watcher')
    owner.registerFunction(`eq::price-${RUN}`, async (input: { sku: string; qty: number; fail?: boolean }) => {
      if (input.fail) throw new Error('out of stock')
      return { total: input.qty * 10, sku: input.sku }
    })
    owner.registerFunction(`eq::checkout-${RUN}`, async (input: { dispatch: 'auto' | 'remote'; fail?: boolean }) =>
      owner.trigger({
        function_id: `eq::price-${RUN}`,
        payload: { sku: 'A-1', qty: 3, fail: input.fail },
        metadata: { channel: 'web' },
        dispatch: input.dispatch,
      }),
    )
    const got = collector(watcher, `eq::on-price-${RUN}`)
    const handle = watcher.observe(
      {
        function_id: `eq::price-${RUN}`,
        on: 'complete',
        capture: [...ALL],
        filters: { 'input.sku': { $prefix: 'A-' } },
      },
      `eq::on-price-${RUN}`,
      { metadata: { observer: 'eq' } },
    )
    await handle.ready()
    await localDispatchReady(owner, `eq::price-${RUN}`)
    await localDispatchReady(owner, `eq::checkout-${RUN}`)

    const frames = frameCounter(owner)
    const run = async (dispatch: 'auto' | 'remote', fail: boolean) => {
      const before = got.length
      await owner
        .trigger({ function_id: `eq::checkout-${RUN}`, payload: { dispatch, fail }, dispatch })
        .catch(() => undefined)
      await waitFor(() => got.length > before, `the ${dispatch} activation`)
      return got[got.length - 1]
    }

    const remoteOk = await run('remote', false)
    const remoteFrames = frames.types.filter((t) => t === 'invokefunction').length
    const localOk = await run('auto', false)
    expect(frames.types.filter((t) => t === 'invokefunction').length).toBe(remoteFrames) // no invocation frame
    const remoteErr = await run('remote', true)
    const localErr = await run('auto', true)

    expect(stable(localOk.data)).toEqual(stable(remoteOk.data))
    expect(stable(localErr.data)).toEqual(stable(remoteErr.data))
    expect(localOk.metadata).toEqual(remoteOk.metadata)
    expect(localOk.inv?.observe_depth).toBe(remoteOk.inv?.observe_depth)
    expect(remoteOk.data).toMatchObject({
      input: { sku: 'A-1', qty: 3 },
      output: { total: 30, sku: 'A-1' },
      metadata: { channel: 'web' },
      caller: { function_id: `eq::checkout-${RUN}` },
    })
  })
})

describe('zero-wire and the activation race', () => {
  it('unobserved local calls send zero frames', { timeout: 60_000 }, async () => {
    // Zero frames is the property under test; the unit suites already run 1M
    // calls against the fake engine. Against a live engine the heartbeat and
    // OTel run alongside, and 1M calls took 31.7 s on a shared CI runner, so
    // this count keeps the run to a few seconds. The time bound still catches
    // calls that took the engine round trip instead.
    const CALLS = 100_000
    const owner = worker('zero-wire')
    owner.registerFunction(`zw::inc-${RUN}`, async (n: number) => n + 1)
    await localDispatchReady(owner, `zw::inc-${RUN}`)
    const frames = frameCounter(owner)
    const started = Date.now()
    for (let i = 0; i < CALLS; i++) await owner.trigger({ function_id: `zw::inc-${RUN}`, payload: i })
    const elapsed = Date.now() - started
    expect(frames.types.filter((t) => t !== 'localinvocationstats')).toEqual([])
    expect(elapsed).toBeLessThan(30_000)

    // Positive control, same worker and counter: once observed, local calls do reach the wire.
    const watcher = worker('zero-wire-watcher')
    const got = collector(watcher, `zw::target-${RUN}`)
    const handle = watcher.observe({ function_id: `zw::inc-${RUN}`, capture: ['input'] }, `zw::target-${RUN}`)
    await handle.ready()
    for (let i = 0; i < 10; i++) await owner.trigger({ function_id: `zw::inc-${RUN}`, payload: i })
    await waitFor(() => got.length === 10, 'the ten activations')
    expect(frames.types.filter((t) => t === 'invocationsobserved').length).toBeGreaterThan(0)
  })

  it('after ready() resolves, every local invocation is reported (exactly N activations)', async () => {
    const N = 200
    const owner = worker('race-owner')
    const watcher = worker('race-watcher')
    owner.registerFunction(`race::fn-${RUN}`, async (n: number) => n)
    await localDispatchReady(owner, `race::fn-${RUN}`)
    const got = collector(watcher, `race::target-${RUN}`)
    const handle = watcher.observe({ function_id: `race::fn-${RUN}`, capture: ['input'] }, `race::target-${RUN}`, {
      max_in_flight: N,
    })
    await handle.ready()
    const frames = frameCounter(owner)
    for (let i = 0; i < N; i++) await owner.trigger({ function_id: `race::fn-${RUN}`, payload: i })
    await exactly(got, N, `${N} activations`, 10_000)
    expect(new Set(got.map((g) => g.data.input)).size).toBe(N)
    expect(frames.types).not.toContain('invokefunction')
    expect(frames.types).toContain('invocationsobserved')
  })
})

describe('owner-delivered functions', () => {
  type Tick = { expression: string; n: number }

  /** A cron-like owner: one index entry per expression, ticks emitted to that expression's observers. */
  function cronOwner(role = 'cron-owner') {
    // A function id of its own: observers of an earlier test's owner outlive it (suspended, 30 s grace).
    const fn = `cron::on-tick-${crypto.randomUUID().slice(0, 8)}`
    const owner = worker(role)
    const index = new Map<string, Set<Observer>>()
    const added: Observer[] = []
    const removed: Observer[] = []
    const emitted: boolean[] = []
    let reject: string | undefined
    let addDelayMs = 0
    const ref = owner.registerFunction(
      fn,
      async (tick: Tick) => {
        for (const obs of index.get(tick.expression) ?? []) emitted.push(obs.emit(tick))
        return null
      },
      {
        observe: {
          delivery: 'owner',
          args_format: {
            type: 'object',
            additionalProperties: false,
            required: ['expression'],
            properties: { expression: { type: 'string' } },
          },
          onObserverAdded: async (obs) => {
            added.push(obs)
            if (addDelayMs) await sleep(addDelayMs)
            if (reject && obs.args.expression === reject) {
              throw new ObserveRejection('INVALID_CRON_EXPRESSION', `bad expression ${reject}`)
            }
            const expression = obs.args.expression as string
            if (!index.has(expression)) index.set(expression, new Set())
            index.get(expression)?.add(obs)
          },
          onObserverRemoved: (obs) => {
            removed.push(obs)
            index.get(obs.args.expression as string)?.delete(obs)
          },
        },
      },
    )
    return {
      fn,
      owner,
      ref,
      index,
      added,
      removed,
      emitted,
      rejectExpression: (e: string) => {
        reject = e
      },
      slowAdds: (ms: number) => {
        addDelayMs = ms
      },
    }
  }

  it('args: obs.args is what the observer sent; a missing required arg and args on an auto function are INVALID_ARGS before any hook', async () => {
    const cron = cronOwner()
    const watcher = worker('args-watcher')
    watcher.registerFunction(`args::target-${RUN}`, async () => null)
    watcher.registerFunction(`args::auto-${RUN}`, async () => null)
    const ok = watcher.observe({ function_id: cron.fn, args: { expression: '*/5' } }, `args::target-${RUN}`)
    await ok.ready()
    expect(cron.added.map((o) => o.args)).toEqual([{ expression: '*/5' }])

    const missing = watcher.observe({ function_id: cron.fn, args: {} }, `args::target-${RUN}`)
    await expect(missing.ready()).rejects.toMatchObject({ code: 'INVALID_ARGS' })
    const onAuto = watcher.observe(
      { function_id: `args::auto-${RUN}`, args: { expression: '*/5' } },
      `args::target-${RUN}`,
    )
    await expect(onAuto.ready()).rejects.toMatchObject({ code: 'INVALID_ARGS' })
    expect(cron.added).toHaveLength(1)
  })

  it('routing: two observers with different args only receive what the owner emits to them; an emit right after ready() is delivered', async () => {
    const cron = cronOwner()
    const watcher = worker('routing-watcher')
    const five = collector(watcher, `routing::five-${RUN}`)
    const ten = collector(watcher, `routing::ten-${RUN}`)
    const a = watcher.observe({ function_id: cron.fn, args: { expression: '*/5' } }, `routing::five-${RUN}`)
    const b = watcher.observe({ function_id: cron.fn, args: { expression: '*/10' } }, `routing::ten-${RUN}`)
    await Promise.all([a.ready(), b.ready()])
    await localDispatchReady(cron.owner, cron.fn)
    // right after ready(): the owner indexed the observer before acking it
    await cron.owner.trigger({ function_id: cron.fn, payload: { expression: '*/10', n: 1 } })
    await cron.owner.trigger({ function_id: cron.fn, payload: { expression: '*/5', n: 2 } })
    await cron.owner.trigger({ function_id: cron.fn, payload: { expression: '* * *', n: 3 } })
    await exactly(ten, 1, 'the */10 activation')
    await exactly(five, 1, 'the */5 activation')
    expect(ten[0].data).toMatchObject({ observer_id: b.id, function_id: cron.fn, input: { n: 1 } })
    expect(five[0].data).toMatchObject({ observer_id: a.id, input: { n: 2 } })
  })

  it('hooks: once per add and before the ack; a throw rejects ready() with its code; the engine keeps the filters; onObserverRemoved on dispose', async () => {
    const cron = cronOwner()
    cron.rejectExpression('bad')
    cron.slowAdds(300)
    const watcher = worker('hooks-watcher')
    watcher.registerFunction(`hooks::target-${RUN}`, async () => null)
    const filters = { 'input.n': { $gte: 2 } }
    const ok = watcher.observe({ function_id: cron.fn, args: { expression: '*/5' }, filters }, `hooks::target-${RUN}`)
    const startedAt = Date.now()
    await ok.ready()
    expect(Date.now() - startedAt).toBeGreaterThanOrEqual(250) // active only after the hook resolved
    expect(cron.added.filter((o) => o.id === ok.id)).toHaveLength(1)
    expect(cron.added[0].filters).toEqual({ $and: [{ 'input.n': { $gte: 2 } }] })

    const got = await watcher.trigger<{ id: string }, Record<string, unknown>>({
      function_id: 'engine::observers::get',
      payload: { id: ok.id },
    })
    expect(JSON.stringify(got)).toContain('"input.n"')

    const bad = watcher.observe({ function_id: cron.fn, args: { expression: 'bad' } }, `hooks::target-${RUN}`)
    await expect(bad.ready()).rejects.toMatchObject({ code: 'INVALID_CRON_EXPRESSION' })
    expect(bad.state).toBe('error')

    await ok.dispose()
    await waitFor(() => cron.removed.some((o) => o.id === ok.id), 'onObserverRemoved after dispose')
    expect(cron.ref.observers().map((o) => o.id)).not.toContain(ok.id)
  })

  it('obs.emit: false and no frame when the filters reject the event; the target sees the envelope and the metadata sidecar', async () => {
    const cron = cronOwner()
    const watcher = worker('emit-watcher')
    const got = collector(watcher, `emit::target-${RUN}`)
    const handle = watcher.observe(
      {
        function_id: cron.fn,
        args: { expression: '*/5' },
        filters: { 'input.n': { $gt: 10 } },
        capture: ['input', 'caller'],
      },
      `emit::target-${RUN}`,
      { metadata: { __binding: 'digest' } },
    )
    await handle.ready()
    await localDispatchReady(cron.owner, cron.fn)
    const frames = frameCounter(cron.owner)
    await cron.owner.trigger({ function_id: cron.fn, payload: { expression: '*/5', n: 1 } })
    await sleep(100)
    expect(cron.emitted).toEqual([false])
    expect(frames.count).toBe(0)

    await cron.owner.trigger({ function_id: cron.fn, payload: { expression: '*/5', n: 11 } })
    await exactly(got, 1, 'the emitted activation')
    expect(got[0].data).toMatchObject({
      observer_id: handle.id,
      status: 'success',
      input: { expression: '*/5', n: 11 },
      caller: { worker_name: expect.stringContaining('cron-owner') },
    })
    expect(got[0].metadata).toEqual({ __binding: 'digest' })
  })

  it("obs.call: resolves with the target's result; FILTERS_NOT_MATCHED sends nothing; OBSERVER_NOT_ACTIVE after dispose; NOT_OWNER_DELIVERED on an auto function", async () => {
    const owner = worker('http-owner')
    const routes = new Map<string, Observer>()
    let autoObs: Observer | undefined
    owner.registerFunction(
      `http::on-request-${RUN}`,
      async (req: { path: string; tenant: string }) => {
        const obs = routes.get(req.path)
        return obs ? obs.call(req) : { status: 404 }
      },
      {
        observe: {
          delivery: 'owner',
          args_format: { type: 'object', required: ['path'], properties: { path: { type: 'string' } } },
          onObserverAdded: (obs) => {
            routes.set(obs.args.path as string, obs)
          },
          onObserverRemoved: (obs) => {
            routes.delete(obs.args.path as string)
          },
        },
      },
    )
    owner.registerFunction(`http::auto-${RUN}`, async () => null, {
      observe: {
        onObserverAdded: (obs) => {
          autoObs = obs
        },
      },
    })
    const app = worker('http-app')
    let metadataSeen: unknown
    app.registerFunction(`users::get-${RUN}`, async (req: { path: string }, metadata: unknown) => {
      metadataSeen = metadata
      return { status: 200, body: { path: req.path } }
    })
    const route = app.observe(
      {
        function_id: `http::on-request-${RUN}`,
        args: { path: '/users' },
        filters: { 'input.tenant': { $eq: 'acme' } },
      },
      `users::get-${RUN}`,
      { metadata: { route: 'users' } },
    )
    const onAuto = app.observe({ function_id: `http::auto-${RUN}` }, `users::get-${RUN}`)
    await Promise.all([route.ready(), onAuto.ready()])
    await localDispatchReady(owner, `http::on-request-${RUN}`)

    await expect(
      owner.trigger({ function_id: `http::on-request-${RUN}`, payload: { path: '/users', tenant: 'acme' } }),
    ).resolves.toEqual({ status: 200, body: { path: '/users' } })
    expect(metadataSeen).toEqual({ route: 'users' })

    const obs = routes.get('/users') as Observer
    const frames = frameCounter(owner)
    await expect(obs.call({ path: '/users', tenant: 'other' })).rejects.toMatchObject({ code: 'FILTERS_NOT_MATCHED' })
    expect(frames.count).toBe(0)

    expect(() => autoObs?.emit({})).toThrow(/NOT_OWNER_DELIVERED/)
    await expect(autoObs?.call({})).rejects.toMatchObject({ code: 'NOT_OWNER_DELIVERED' })

    await route.dispose()
    await waitFor(() => !routes.has('/users'), 'onObserverRemoved')
    await expect(obs.call({ path: '/users', tenant: 'acme' })).rejects.toMatchObject({ code: 'OBSERVER_NOT_ACTIVE' })
  })
})

describe('reconnect', () => {
  it('local dispatch keeps serving, reports flush after the reconnect, and the functionregistered diff calls the right hooks', async () => {
    const owner = worker('rc-owner')
    const watcher = worker('rc-watcher')
    owner.registerFunction(`rc::fn-${RUN}`, async (n: number) => n * 2)
    const got = collector(watcher, `rc::target-${RUN}`)
    const kept = watcher.observe({ function_id: `rc::fn-${RUN}`, capture: ['input'] }, `rc::target-${RUN}`)
    await kept.ready()
    await localDispatchReady(owner, `rc::fn-${RUN}`)

    dropConnection(owner)
    await waitFor(() => owner.getConnectionState() !== 'connected', 'the disconnect')
    await expect(owner.trigger({ function_id: `rc::fn-${RUN}`, payload: 21 })).resolves.toBe(42) // still local
    await waitFor(() => owner.getConnectionState() === 'connected', 'the reconnect')
    await exactly(got, 1, 'the activation reported after the reconnect', 10_000)
    expect(got[0].data.input).toBe(21)

    // the diff: an observer removed while the owner was away
    const cron = (() => {
      const added: string[] = []
      const removed: string[] = []
      owner.registerFunction(`rc::source-${RUN}`, async () => null, {
        observe: {
          onObserverAdded: (obs) => {
            added.push(obs.id)
          },
          onObserverRemoved: (obs) => {
            removed.push(obs.id)
          },
        },
      })
      return { added, removed }
    })()
    const gone = watcher.observe({ function_id: `rc::source-${RUN}` }, `rc::target-${RUN}`)
    const stays = watcher.observe({ function_id: `rc::source-${RUN}` }, `rc::target-${RUN}`)
    await Promise.all([gone.ready(), stays.ready()])
    expect(cron.added.sort()).toEqual([gone.id, stays.id].sort())

    dropConnection(owner)
    await gone.dispose()
    await waitFor(() => owner.getConnectionState() === 'connected', 'the reconnect')
    await waitFor(() => cron.removed.includes(gone.id), 'onObserverRemoved from the diff')
    expect(cron.removed).toEqual([gone.id])
    expect(cron.added.filter((id) => id === stays.id)).toHaveLength(1) // kept: no second add
  })
})
