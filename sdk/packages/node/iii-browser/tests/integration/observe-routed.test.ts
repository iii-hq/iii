// Derived from sdk/packages/node/iii/tests/observe-e2e-routed.test.ts by scripts/sync-browser-observe.py; edit those, not this file.
import { afterEach, describe, expect, it } from 'vitest'
import {
  closeWorkers,
  collector,
  connectionState,
  dropConnection,
  sleep,
  traceId,
  waitFor,
  worker,
} from './observe-utils'

/**
 * `iii.observe` against the real engine, on the routed path (README §7 phase 1):
 * every call goes through the engine, which evaluates the filters.
 */
afterEach(closeWorkers)

// Function ids of this run: observers of an earlier run's workers outlive them (suspended, 30 s grace).
const RUN = crypto.randomUUID().slice(0, 8)

const ALL = ['input', 'output', 'error', 'caller', 'metadata'] as const

describe('observe: routed invocations (engine-evaluated)', () => {
  it('activates the target with the envelope when the filters on input, output and caller match', async () => {
    const owner = worker('routed-owner')
    const watcher = worker('routed-watcher')
    owner.registerFunction(`e2e::charge-${RUN}`, async (input: { amount: number }) => ({ charged: input.amount }))
    const got = collector(watcher, `e2e::on-big-charge-${RUN}`)
    watcher.registerFunction(`e2e::checkout-${RUN}`, async (amount: number) =>
      watcher.trigger({ function_id: `e2e::charge-${RUN}`, payload: { amount } }),
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

    await watcher.trigger({ function_id: `e2e::checkout-${RUN}`, payload: 750 })
    await watcher.trigger({ function_id: `e2e::checkout-${RUN}`, payload: 10 }) // filtered out by input
    await watcher.trigger({ function_id: `e2e::charge-${RUN}`, payload: { amount: 900 } }) // no caller.function_id
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

    await watcher.trigger({ function_id: `e2e::flaky-${RUN}`, payload: { fail: false, n: 1 } })
    await watcher.trigger({ function_id: `e2e::flaky-${RUN}`, payload: { fail: true, n: 2 } }).catch(() => undefined)
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

  it('a target that is not registered keeps the observer pending until it is', async () => {
    const owner = worker('routed-owner')
    const watcher = worker('routed-watcher')
    owner.registerFunction(`e2e::tick-${RUN}`, async () => 'ok')
    const handle = watcher.observe({ function_id: `e2e::tick-${RUN}` }, `e2e::late-target-${RUN}`)
    await sleep(300)
    expect(handle.state).toBe('pending')
    const got = collector(watcher, `e2e::late-target-${RUN}`)
    await handle.ready()
    await watcher.trigger({ function_id: `e2e::tick-${RUN}`, payload: {} })
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
    await watcher.trigger({ function_id: `e2e::ping-${RUN}`, payload: {} })
    await watcher.trigger({ function_id: `e2e::ping-${RUN}`, payload: {} })
    await waitFor(() => b.state === 'disposed', 'max_fires to dispose the observer')
    await sleep(200)
    expect(disposed).toHaveLength(0)
    expect(once).toHaveLength(1)
    expect(b.reason?.code).toBe('MAX_FIRES')
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
    await owner.trigger({ function_id: `e2e::work-${RUN}`, payload: {} })
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
    await owner.trigger({ function_id: `e2e::lease-${RUN}`, payload: {} })
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
      await watcher.trigger({ function_id: `e2e::loop-${RUN}`, payload: envelope.input + 1 })
      return null
    })
    const handle = watcher.observe({ function_id: `e2e::loop-${RUN}`, capture: ['input'] }, `e2e::on-loop-${RUN}`)
    await handle.ready()
    await watcher.trigger({ function_id: `e2e::loop-${RUN}`, payload: 0 })
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
      await watcher.trigger({ function_id: `e2e::await-loop-${RUN}`, payload: envelope.input + 1 })
      return null
    })
    const handle = watcher.observe(
      { function_id: `e2e::await-loop-${RUN}`, capture: ['input'] },
      `e2e::on-await-loop-${RUN}`,
    )
    await handle.ready()
    await watcher.trigger({ function_id: `e2e::await-loop-${RUN}`, payload: 0 })
    await waitFor(() => depths.length >= 8, 'eight hops')
    await sleep(1000)
    expect(depths).toEqual([1, 2, 3, 4, 5, 6, 7, 8])
  })

  it('trigger sends metadata and traceparent: filters read metadata.*, and the activation continues the trace', async () => {
    // Browser SDK: there is no OpenTelemetry, so the trace context is what trigger() is given.
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
    await watcher.trigger({
      function_id: `e2e::tag-${RUN}`,
      payload: { n: 1 },
      metadata: { tenant: 'acme' },
      traceparent,
    })
    await watcher.trigger({ function_id: `e2e::tag-${RUN}`, payload: { n: 2 }, metadata: { tenant: 'other' } })
    await waitFor(() => got.length >= 1, 'the activation')
    await sleep(200)
    expect(got).toHaveLength(1)
    expect(got[0].data).toMatchObject({ input: { n: 1 }, metadata: { tenant: 'acme' } })
    expect(traceId(got[0].inv?.traceparent)).toBe(traceId(traceparent))
  })
})
