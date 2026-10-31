// Derived from sdk/packages/node/iii/tests/observe-e2e-routed.test.ts by scripts/sync-browser-observe.py; edit those, not this file.
import { afterEach, describe, expect, it } from 'vitest'
import {
  closeWorkers,
  collector,
  connectionState,
  dropConnection,
  frameCounter,
  sleep,
  traceId,
  waitFor,
  worker,
} from './observe-utils'

/**
 * `iii.observe` against the real engine, on the routed path: every call goes
 * through the engine, which evaluates the observers' `on` and filters
 * (decision 55). The worker that hosts the function runs its match and
 * deliver hooks, and its answer says how the call is delivered.
 */
afterEach(closeWorkers)

// Function ids of this run: observers of an earlier run's workers outlive them (suspended, 30 s grace).
const RUN = crypto.randomUUID().slice(0, 8)

const ALL = ['input', 'output', 'error', 'caller', 'metadata'] as const

describe('observe: routed invocations (filters evaluated by the engine)', () => {
  it('activates the target with the envelope when the filters on input, output and caller match', async () => {
    const owner = worker('routed-owner')
    const watcher = worker('routed-watcher')
    owner.registerFunction(`e2e::charge-${RUN}`, async (input: { amount: number }) => ({ charged: input.amount }))
    const got = collector(watcher, `e2e::on-big-charge-${RUN}`)
    watcher.registerFunction(`e2e::checkout-${RUN}`, async (amount: number) =>
      watcher.call({ function_id: `e2e::charge-${RUN}`, payload: { amount } }),
    )
    const handle = watcher.observe(
      {
        function_id: `e2e::charge-${RUN}`,
        filters: {
          'input.amount': { $gte: 500 },
          'output.charged': { $exists: true },
          'caller.function_id': { $eq: `e2e::checkout-${RUN}` },
        },
        capture: [...ALL],
      },
      `e2e::on-big-charge-${RUN}`,
      { metadata: { team: 'payments' } },
    )
    await handle.ready()
    expect(handle.state).toBe('active')

    await watcher.call({ function_id: `e2e::checkout-${RUN}`, payload: 750 })
    await watcher.call({ function_id: `e2e::checkout-${RUN}`, payload: 10 }) // filtered out by input
    await watcher.call({ function_id: `e2e::charge-${RUN}`, payload: { amount: 900 } }) // no caller.function_id
    await waitFor(() => got.length >= 1, 'one activation')
    await sleep(200)
    expect(got).toHaveLength(1)
    const [{ data, metadata, inv }] = got
    expect(data).toMatchObject({
      observer_id: handle.id,
      function_id: `e2e::charge-${RUN}`,
      namespace: 'default',
      status: 'success',
      input: { amount: 750 },
      output: { charged: 750 },
      caller: { worker_name: expect.stringContaining('routed-watcher'), function_id: `e2e::checkout-${RUN}` },
    })
    expect(typeof data.activation_id).toBe('string')
    expect(typeof data.invocation_id).toBe('string')
    expect(typeof data.started_at_ms).toBe('number')
    expect(typeof data.duration_ms).toBe('number')
    expect(metadata).toEqual({ team: 'payments' }) // the observer's metadata sidecar
    expect(inv).toMatchObject({
      function_id: `e2e::on-big-charge-${RUN}`,
      observe_depth: 1,
      caller: { worker_name: 'iii-engine', function_id: `e2e::charge-${RUN}`, invocation_id: data.invocation_id },
      activation: { observer_id: handle.id, activation_id: data.activation_id },
    })
  })

  it('the host answers with the result alone and sends no report: the engine activates the observer', async () => {
    const owner = worker('routed-owner')
    const watcher = worker('routed-watcher')
    owner.registerFunction(`e2e::quote-${RUN}`, async (qty: number) => ({ total: qty * 3 }))
    const got = collector(watcher, `e2e::on-quote-${RUN}`)
    const handle = watcher.observe({ function_id: `e2e::quote-${RUN}` }, `e2e::on-quote-${RUN}`)
    await handle.ready()
    const frames = frameCounter(owner)
    await expect(watcher.call({ function_id: `e2e::quote-${RUN}`, payload: 2 })).resolves.toEqual({ total: 6 })
    await waitFor(() => got.length === 1, 'the activation')
    const answer = frames.frames.find((f) => f.type === 'invocationresult')
    expect(answer).toBeDefined()
    expect(answer).not.toHaveProperty('matched')
    expect(answer).not.toHaveProperty('delivered')
    expect(frames.types).not.toContain('invocationsobserved')
    expect(got[0].data).toMatchObject({ input: 2, output: { total: 6 } })
  })

  it('a match hook names the observers on the answer (matched); the engine evaluates their filters', async () => {
    const owner = worker('routed-owner')
    const watcher = worker('routed-watcher')
    owner
      .registerFunction(`e2e::ship-${RUN}`, async (input: { region: string; weight: number }) => input, {
        args_format: { type: 'object', properties: { region: { type: 'string' } } },
      })
      .onObserverMatch({ observer: (obs) => obs.args.region as string, call: (call) => call.input.region })
    const eu = collector(watcher, `e2e::on-eu-${RUN}`)
    const heavy = collector(watcher, `e2e::on-eu-heavy-${RUN}`)
    const us = collector(watcher, `e2e::on-us-${RUN}`)
    const handles = [
      watcher.observe({ function_id: `e2e::ship-${RUN}`, args: { region: 'eu' } }, `e2e::on-eu-${RUN}`),
      watcher.observe(
        { function_id: `e2e::ship-${RUN}`, args: { region: 'eu' }, filters: { 'input.weight': { $gte: 10 } } },
        `e2e::on-eu-heavy-${RUN}`,
      ),
      watcher.observe({ function_id: `e2e::ship-${RUN}`, args: { region: 'us' } }, `e2e::on-us-${RUN}`),
    ]
    await Promise.all(handles.map((h) => h.ready()))
    const frames = frameCounter(owner)
    await watcher.call({ function_id: `e2e::ship-${RUN}`, payload: { region: 'eu', weight: 2 } })
    await watcher.call({ function_id: `e2e::ship-${RUN}`, payload: { region: 'eu', weight: 12 } })
    await waitFor(() => eu.length >= 2 && heavy.length >= 1, 'the activations')
    await sleep(200)
    expect(eu.map((a) => a.data.input)).toEqual([
      { region: 'eu', weight: 2 },
      { region: 'eu', weight: 12 },
    ])
    expect(heavy.map((a) => a.data.input)).toEqual([{ region: 'eu', weight: 12 }])
    expect(us).toHaveLength(0)
    const answers = frames.frames.filter((f) => f.type === 'invocationresult')
    expect(answers.map((a) => (a.matched as string[]).sort())).toEqual([
      [handles[0].id, handles[1].id].sort(),
      [handles[0].id, handles[1].id].sort(),
    ])
  })

  it('a deliver hook answers delivered: the engine activates nothing for the call', async () => {
    const owner = worker('routed-owner')
    const watcher = worker('routed-watcher')
    owner
      .registerFunction(`e2e::ask-${RUN}`, async (q: string) => q)
      .onObserverDeliver(async (call, observers) => {
        const [obs] = observers
        if (!obs) return 'nobody'
        return owner.call({ observer: obs, payload: call.input })
      })
    const plain = collector(watcher, `e2e::on-ask-${RUN}`)
    const handle = watcher.observe({ function_id: `e2e::ask-${RUN}` }, `e2e::on-ask-${RUN}`)
    await handle.ready()
    const frames = frameCounter(owner)
    await watcher.call({ function_id: `e2e::ask-${RUN}`, payload: 'q-1' })
    await waitFor(() => plain.length === 1, 'the call through the observer')
    await sleep(200)
    // the target received the hook's call through the observer, not an activation envelope
    expect(plain).toHaveLength(1)
    expect(plain[0].data).toBe('q-1')
    const answer = frames.frames.find((f) => f.type === 'invocationresult' && f.function_id === `e2e::ask-${RUN}`)
    expect(answer).toMatchObject({ delivered: true })
    expect(answer).not.toHaveProperty('matched')
  })

  it("on: 'error' sees failures only; capture limits what travels; deliver: 'input' sends the input alone", async () => {
    const owner = worker('routed-owner')
    const watcher = worker('routed-watcher')
    owner.registerFunction(`e2e::flaky-${RUN}`, async (input: { fail: boolean; n: number }) => {
      if (input.fail) throw new Error('flaky failed')
      return input.n
    })
    const errors = collector(watcher, `e2e::on-error-${RUN}`)
    const inputs = collector(watcher, `e2e::on-input-${RUN}`)
    const a = watcher.observe(
      { function_id: `e2e::flaky-${RUN}`, on: 'error', capture: ['error'] },
      `e2e::on-error-${RUN}`,
    )
    const b = watcher.observe(
      { function_id: `e2e::flaky-${RUN}`, on: 'complete', deliver: 'input' },
      `e2e::on-input-${RUN}`,
    )
    await Promise.all([a.ready(), b.ready()])

    await watcher.call({ function_id: `e2e::flaky-${RUN}`, payload: { fail: false, n: 1 } })
    await watcher.call({ function_id: `e2e::flaky-${RUN}`, payload: { fail: true, n: 2 } }).catch(() => undefined)
    await waitFor(() => errors.length >= 1 && inputs.length >= 2, 'the activations')
    await sleep(200)
    expect(errors).toHaveLength(1)
    expect(errors[0].data).toMatchObject({
      status: 'error',
      error: { code: 'invocation_failed', message: 'flaky failed' },
    })
    expect(errors[0].data).not.toHaveProperty('input')
    expect(inputs.map((x) => x.data).sort((p, q) => Number(p.n) - Number(q.n))).toEqual([
      { fail: false, n: 1 },
      { fail: true, n: 2 },
    ])
  })

  it("deliver: 'output' sends the handler's return value alone, also when it is the input", async () => {
    const owner = worker('routed-owner')
    const watcher = worker('routed-watcher')
    owner.registerFunction(`e2e::price-${RUN}`, async (input: { qty: number }) => ({ total: input.qty * 4 }))
    owner.registerFunction(`e2e::echo-${RUN}`, async (input: { n: number }) => input)
    const prices = collector(watcher, `e2e::on-price-output-${RUN}`)
    const echoes = collector(watcher, `e2e::on-echo-output-${RUN}`)
    const a = watcher.observe({ function_id: `e2e::price-${RUN}`, deliver: 'output' }, `e2e::on-price-output-${RUN}`)
    const b = watcher.observe({ function_id: `e2e::echo-${RUN}`, deliver: 'output' }, `e2e::on-echo-output-${RUN}`)
    await Promise.all([a.ready(), b.ready()])

    await expect(watcher.call({ function_id: `e2e::price-${RUN}`, payload: { qty: 2 } })).resolves.toEqual({
      total: 8,
    })
    await watcher.call({ function_id: `e2e::echo-${RUN}`, payload: { n: 1 } })
    await waitFor(() => prices.length >= 1 && echoes.length >= 1, 'the activations')
    await sleep(200)
    // exactly the return value: no envelope, no engine stamp (`_caller_worker_id`)
    expect(prices.map((x) => x.data)).toEqual([{ total: 8 }])
    expect(echoes.map((x) => x.data)).toEqual([{ n: 1 }])
  })

  it('a target that is not registered keeps the observer pending until it is', async () => {
    const owner = worker('routed-owner')
    const watcher = worker('routed-watcher')
    owner.registerFunction(`e2e::tick-${RUN}`, async () => 'ok')
    const handle = watcher.observe({ function_id: `e2e::tick-${RUN}` }, `e2e::late-target-${RUN}`)
    await sleep(300)
    expect(handle.state).toBe('pending')
    const got = collector(watcher, `e2e::late-target-${RUN}`)
    await handle.ready()
    await watcher.call({ function_id: `e2e::tick-${RUN}`, payload: {} })
    await waitFor(() => got.length === 1, 'the activation')
  })

  it.each([
    ['SELF_TARGET', { function_id: `e2e::self-${RUN}` }, `e2e::self-${RUN}`],
    ['INVALID_ARGS', { function_id: `e2e::self-${RUN}`, args: { expression: '* * *' } }, `e2e::other-${RUN}`],
    ['INVALID_FILTER', { function_id: `e2e::self-${RUN}`, filters: { 'input.op': 'insert' } }, `e2e::other-${RUN}`],
  ] as const)('%s turns the observer to error and rejects ready()', async (code, source, target) => {
    const owner = worker('routed-owner')
    owner.registerFunction(`e2e::self-${RUN}`, async () => null)
    owner.registerFunction(`e2e::other-${RUN}`, async () => null)
    const handle = owner.observe(source as never, target)
    await expect(handle.ready()).rejects.toMatchObject({ code })
    expect(handle.state).toBe('error')
    expect(handle.reason?.code).toBe(code)
  })

  it('dispose stops the activations; max_fires disposes after the last one', async () => {
    const owner = worker('routed-owner')
    const watcher = worker('routed-watcher')
    owner.registerFunction(`e2e::ping-${RUN}`, async () => 'pong')
    const disposed = collector(watcher, `e2e::on-ping-a-${RUN}`)
    const once = collector(watcher, `e2e::on-ping-b-${RUN}`)
    const a = watcher.observe({ function_id: `e2e::ping-${RUN}` }, `e2e::on-ping-a-${RUN}`)
    const b = watcher.observe({ function_id: `e2e::ping-${RUN}` }, `e2e::on-ping-b-${RUN}`, { max_fires: 1 })
    await Promise.all([a.ready(), b.ready()])
    await a.dispose()
    expect(a.state).toBe('disposed')
    await watcher.call({ function_id: `e2e::ping-${RUN}`, payload: {} })
    await watcher.call({ function_id: `e2e::ping-${RUN}`, payload: {} })
    await waitFor(() => b.state === 'disposed', 'max_fires to dispose the observer')
    await sleep(200)
    expect(disposed).toHaveLength(0)
    expect(once).toHaveLength(1)
    expect(b.reason?.code).toBe('MAX_FIRES')
  })

  it('an id observed again at once after dispose: the new observer stays active, and its dispose stops it (decision 56)', async () => {
    const owner = worker('routed-owner')
    const watcher = worker('routed-watcher')
    owner.registerFunction(`e2e::reuse-${RUN}`, async (n: number) => ({ n }))
    const got = collector(watcher, `e2e::on-reuse-${RUN}`)
    const frames = frameCounter(watcher)
    const id = `reuse-${RUN}`
    const first = watcher.observe({ function_id: `e2e::reuse-${RUN}` }, `e2e::on-reuse-${RUN}`, { id })
    await first.ready()
    await first.dispose()
    const second = watcher.observe({ function_id: `e2e::reuse-${RUN}` }, `e2e::on-reuse-${RUN}`, { id })
    await second.ready()
    // The earlier observer's disposed, sent before the engine read the new observe, came and went.
    await sleep(200)
    expect(second.state).toBe('active')
    expect(first.reason?.code).toBe('UNOBSERVED')
    await watcher.call({ function_id: `e2e::reuse-${RUN}`, payload: 1 })
    await waitFor(() => got.length === 1, 'an activation through the new observer')
    expect(got[0].data.observer_id).toBe(id)

    await second.dispose()
    const sent = frames.frames.filter((f) => f.type === 'observe' || f.type === 'unobserve').map((f) => f.type)
    expect(sent).toEqual(['observe', 'unobserve', 'observe', 'unobserve'])
    await expect(watcher.call({ function_id: 'engine::observers::get', payload: { id } })).rejects.toMatchObject({
      code: 'OBSERVER_NOT_FOUND',
    })
    await watcher.call({ function_id: `e2e::reuse-${RUN}`, payload: 2 })
    await sleep(300)
    expect(got).toHaveLength(1)
  })

  it('a refused observe: its dispose sends nothing, and the id observed again works', async () => {
    const owner = worker('routed-owner')
    const watcher = worker('routed-watcher')
    owner.registerFunction(`e2e::refused-${RUN}`, async (n: number) => ({ n }))
    const got = collector(watcher, `e2e::on-refused-${RUN}`)
    const frames = frameCounter(watcher)
    const sent = () => frames.frames.filter((f) => f.type === 'observe' || f.type === 'unobserve').map((f) => f.type)
    const id = `refused-${RUN}`
    const source = { function_id: `e2e::refused-${RUN}` }
    const target = `e2e::on-refused-${RUN}`

    // Refused, then disposed: nothing is sent for it.
    const refused = watcher.observe(source, target, { id, max_fires: 0 })
    await expect(refused.ready()).rejects.toMatchObject({ code: 'INVALID_ARGS' })
    const started = performance.now()
    await refused.dispose()
    expect(performance.now() - started).toBeLessThan(500)
    expect(sent()).toEqual(['observe'])

    // Disposed before its refusal: the unobserve the engine ignores does not touch the next observe of the id.
    const early = watcher.observe(source, target, { id, max_fires: 0 })
    await early.dispose()
    const kept = watcher.observe(source, target, { id })
    await kept.ready()
    expect(sent()).toEqual(['observe', 'observe', 'unobserve', 'observe'])
    await sleep(200)
    expect(kept.state).toBe('active')
    expect(early.reason?.code).toBe('UNOBSERVED')
    await watcher.call({ function_id: `e2e::refused-${RUN}`, payload: 1 })
    await waitFor(() => got.length === 1, 'an activation through the kept observer')
    expect(got[0].data.observer_id).toBe(id)

    await kept.dispose()
    expect(sent()).toEqual(['observe', 'observe', 'unobserve', 'observe', 'unobserve'])
    await expect(watcher.call({ function_id: 'engine::observers::get', payload: { id } })).rejects.toMatchObject({
      code: 'OBSERVER_NOT_FOUND',
    })
  })

  it('a refused observe is not sent again after a reconnect: it stays refused once the id is free', async () => {
    const owner = worker('routed-owner')
    const holder = worker('routed-holder')
    const watcher = worker('routed-watcher')
    owner.registerFunction(`e2e::contested-${RUN}`, async (n: number) => ({ n }))
    collector(holder, `e2e::on-contested-held-${RUN}`)
    const got = collector(watcher, `e2e::on-contested-${RUN}`)
    const id = `contested-${RUN}`
    const held = holder.observe({ function_id: `e2e::contested-${RUN}` }, `e2e::on-contested-held-${RUN}`, { id })
    await held.ready()
    const refused = watcher.observe({ function_id: `e2e::contested-${RUN}` }, `e2e::on-contested-${RUN}`, { id })
    await expect(refused.ready()).rejects.toMatchObject({ code: 'OBSERVER_ID_CONFLICT' })
    await held.dispose()
    const frames = frameCounter(watcher)
    dropConnection(watcher)
    // Its functions go out again on the new connection, and its observers right after them.
    await waitFor(() => frames.types.includes('registerfunction'), 'the re-registration on the new connection')
    await sleep(200)
    await owner.call({ function_id: `e2e::contested-${RUN}`, payload: 1 })
    await sleep(300)
    expect(frames.frames.filter((f) => f.type === 'observe' && f.id === id)).toHaveLength(0)
    expect(refused.state).toBe('error')
    expect(refused.reason?.code).toBe('OBSERVER_ID_CONFLICT')
    expect(got).toHaveLength(0)
    await refused.dispose()
    expect(frames.frames.filter((f) => f.type === 'unobserve')).toHaveLength(0)
  })

  it('an id the engine disposed (max_fires, expires_in_ms) is free at once: observed again, it delivers', async () => {
    const owner = worker('routed-owner')
    const watcher = worker('routed-watcher')
    owner.registerFunction(`e2e::again-${RUN}`, async (n: number) => ({ n }))
    const got = collector(watcher, `e2e::on-again-${RUN}`)
    const source = { function_id: `e2e::again-${RUN}` }
    const target = `e2e::on-again-${RUN}`
    const id = `again-${RUN}`
    for (const round of [1, 2]) {
      const once = watcher.observe(source, target, { id, max_fires: 1 })
      await once.ready()
      await watcher.call({ function_id: `e2e::again-${RUN}`, payload: round })
      await waitFor(() => once.state === 'disposed', `round ${round}: the max_fires disposal`)
      expect(once.reason?.code).toBe('MAX_FIRES')
      await waitFor(() => got.length === round, `round ${round}: its activation`)
    }

    const lease = `lease-${RUN}`
    const first = watcher.observe(source, target, { id: lease, expires_in_ms: 200 })
    await first.ready()
    await waitFor(() => first.state === 'disposed', 'the expiry')
    expect(first.reason?.code).toBe('EXPIRED')
    const renewed = watcher.observe(source, target, { id: lease })
    await renewed.ready()
    await watcher.call({ function_id: `e2e::again-${RUN}`, payload: 3 })
    await waitFor(() => got.length === 3, 'an activation through the renewed observer')
    expect(got[2].data.observer_id).toBe(lease)
    await renewed.dispose()
  })

  it('the observer survives a reconnect of its worker (same id, replayed)', async () => {
    const owner = worker('routed-owner')
    const watcher = worker('routed-watcher')
    owner.registerFunction(`e2e::work-${RUN}`, async () => 'done')
    const got = collector(watcher, `e2e::on-work-${RUN}`)
    const handle = watcher.observe({ function_id: `e2e::work-${RUN}` }, `e2e::on-work-${RUN}`)
    await handle.ready()
    dropConnection(watcher)
    await waitFor(() => connectionState(watcher) === 'connected', 'the reconnect')
    await sleep(500)
    await owner.call({ function_id: `e2e::work-${RUN}`, payload: {} })
    await waitFor(() => got.length === 1, 'an activation after the reconnect')
    expect(got[0].data.observer_id).toBe(handle.id)
    expect(handle.state).toBe('active')
  })

  it('expires_in_ms: an expired observer is disposed and fires no more', async () => {
    const owner = worker('routed-owner')
    const watcher = worker('routed-watcher')
    owner.registerFunction(`e2e::lease-${RUN}`, async () => 'ok')
    const got = collector(watcher, `e2e::on-lease-${RUN}`)
    const handle = watcher.observe({ function_id: `e2e::lease-${RUN}` }, `e2e::on-lease-${RUN}`, { expires_in_ms: 300 })
    await handle.ready()
    await sleep(500)
    await owner.call({ function_id: `e2e::lease-${RUN}`, payload: {} })
    await waitFor(() => handle.state === 'disposed', 'the expiry')
    await sleep(200)
    expect(got).toHaveLength(0)
    expect(handle.reason?.code).toBe('EXPIRED')
  })

  it('the loop guard: observe_depth travels with the calls a target makes and stops an observe loop', async () => {
    // e2e::loop is observed by e2e::on-loop, which calls e2e::loop again: each
    // hop is one observe level deeper (the SDK copies observe_depth from the
    // activation into the call), until the engine drops it (max_observe_depth 8).
    const owner = worker('routed-owner')
    const watcher = worker('routed-watcher')
    owner.registerFunction(`e2e::loop-${RUN}`, async (n: number) => n)
    const depths: number[] = []
    watcher.registerFunction(`e2e::on-loop-${RUN}`, async (envelope: { input: number }) => {
      depths.push(watcher.currentInvocation()?.observe_depth ?? -1)
      await watcher.call({ function_id: `e2e::loop-${RUN}`, payload: envelope.input + 1 })
      return null
    })
    const handle = watcher.observe({ function_id: `e2e::loop-${RUN}`, capture: ['input'] }, `e2e::on-loop-${RUN}`)
    await handle.ready()
    await watcher.call({ function_id: `e2e::loop-${RUN}`, payload: 0 })
    await waitFor(() => depths.length >= 8, 'eight hops')
    await sleep(500)
    expect(depths).toEqual([1, 2, 3, 4, 5, 6, 7, 8])
  })

  it('the loop guard holds for a target that awaits before it calls back', async () => {
    // Browser SDK: the invocation scope ends at the first `await`. The call made
    // after it carries the depth of the activation still running, so the engine
    // stops the loop at max_observe_depth (8) all the same.
    const owner = worker('routed-owner')
    const watcher = worker('routed-watcher')
    owner.registerFunction(`e2e::await-loop-${RUN}`, async (n: number) => n)
    const depths: number[] = []
    watcher.registerFunction(`e2e::on-await-loop-${RUN}`, async (envelope: { input: number }) => {
      depths.push(watcher.currentInvocation()?.observe_depth ?? -1)
      await sleep(5)
      await watcher.call({ function_id: `e2e::await-loop-${RUN}`, payload: envelope.input + 1 })
      return null
    })
    const handle = watcher.observe(
      { function_id: `e2e::await-loop-${RUN}`, capture: ['input'] },
      `e2e::on-await-loop-${RUN}`,
    )
    await handle.ready()
    await watcher.call({ function_id: `e2e::await-loop-${RUN}`, payload: 0 })
    await waitFor(() => depths.length >= 8, 'eight hops')
    await sleep(1000)
    expect(depths).toEqual([1, 2, 3, 4, 5, 6, 7, 8])
  })

  it('call sends metadata and traceparent: filters read metadata.*, and the activation continues the trace', async () => {
    // Browser SDK: there is no OpenTelemetry, so the trace context is what call() is given.
    const owner = worker('routed-owner')
    const watcher = worker('routed-watcher')
    owner.registerFunction(`e2e::tag-${RUN}`, async (input: { n: number }) => input.n)
    const got = collector(watcher, `e2e::on-tag-${RUN}`)
    const handle = watcher.observe(
      {
        function_id: `e2e::tag-${RUN}`,
        filters: { 'metadata.tenant': { $eq: 'acme' } },
        capture: ['input', 'metadata'],
      },
      `e2e::on-tag-${RUN}`,
    )
    await handle.ready()
    const traceparent = '00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01'
    await watcher.call({
      function_id: `e2e::tag-${RUN}`,
      payload: { n: 1 },
      metadata: { tenant: 'acme' },
      traceparent,
    })
    await watcher.call({ function_id: `e2e::tag-${RUN}`, payload: { n: 2 }, metadata: { tenant: 'other' } })
    await waitFor(() => got.length >= 1, 'the activation')
    await sleep(200)
    expect(got).toHaveLength(1)
    expect(got[0].data).toMatchObject({ input: { n: 1 }, metadata: { tenant: 'acme' } })
    expect(traceId(got[0].inv?.traceparent)).toBe(traceId(traceparent))
  })
})
