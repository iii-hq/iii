import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Call, DeliverHook, InvocationContext, Observer } from '../src/observe-types'
import type { IIIClient } from '../src/types'

/**
 * Delivery by the host (v2 §4.4–§4.6, §8.2; decisions 13, 20, 36, 39): the
 * deliver hook and `next`, keyed delivery, `isObserved(call)`, routed calls
 * and calls through an observer, against a fake engine.
 */
vi.mock('ws', async () => (await import('./observe-fake-engine')).wsModule())
vi.resetModules()
const fake = await import('./observe-fake-engine')
const { registerWorker, CallAction, keyed } = await import('../src/index')
const { InvocationError, ObserveRejection } = await import('../src/errors')
const { functionRegistered, observerInfo, observerAdded, observerRemoved, settle, reportedItems, invokeFunction } = fake

type Json = Record<string, unknown>

let sdk: IIIClient | undefined
beforeEach(() => fake.fakeClock())
afterEach(async () => {
  vi.restoreAllMocks()
  await sdk?.shutdown()
  sdk = undefined
  vi.useRealTimers()
})

/** Past the reporter's 5 ms linger. */
const flushed = () => fake.elapse(10)

function worker(features: string[] = fake.ENGINE_FEATURES) {
  sdk = registerWorker('ws://deliver.test', {
    otel: { enabled: false },
    enableMetricsReporting: false,
    workerName: 'cron',
    reconnectionConfig: { initialDelayMs: 10, jitterFactor: 0 },
  })
  const socket = fake.lastSocket()
  socket.open(features)
  return { sdk, socket }
}

type Tick = { expression: string; n: number }

const every = (id: string, expression: string, extra: { filters?: Json; capture?: string[]; on?: string } = {}) =>
  observerInfo(id, `local::${id}`, { args: { expression }, capture: ['input'], ...extra })

/** `cron::on-tick`: an event source whose observers ask for an expression. */
async function cron(observers: Json[], hook: (iii: IIIClient) => void) {
  const { sdk, socket } = worker()
  sdk.registerFunction('cron::on-tick', (tick: Tick) => tick, { private: true })
  hook(sdk)
  socket.deliver(functionRegistered('cron::on-tick', observers, { revision: 1 }))
  await settle()
  socket.clear()
  return { sdk, socket }
}

describe('onObserverDeliver with next', () => {
  it('choose observers, let iii deliver: only the observers passed to next are reported', async () => {
    const { sdk, socket } = await cron([every('five', '*/5'), every('ten', '*/10')], (iii) =>
      iii.onObserverDeliver<Tick, Tick>('cron::on-tick', (call, observers, next) =>
        next(
          call,
          observers.filter((o) => o.args.expression === call.input.expression),
        ),
      ),
    )
    await expect(sdk.call({ function_id: 'cron::on-tick', payload: { expression: '*/10', n: 1 } })).resolves.toEqual({
      expression: '*/10',
      n: 1,
    })
    await flushed()
    expect(reportedItems(socket).map((i) => [i.matched, i.input])).toEqual([[['ten'], { expression: '*/10', n: 1 }]])
  })

  it("what the hook returns or throws is what the caller gets back; next resolves to the handler's output", async () => {
    const { sdk } = worker()
    let nextGave: unknown
    sdk
      .registerFunction('http::on-request', (req: { path: string }) => req)
      .onObserverDeliver(async (call, observers, next) => {
        nextGave = await next(call, observers)
        if (call.input.path === '/boom') throw new Error('no route')
        return { status_code: 404, path: call.input.path }
      })
    fake.lastSocket().deliver(functionRegistered('http::on-request'))
    await settle()
    await expect(sdk.call({ function_id: 'http::on-request', payload: { path: '/x' } })).resolves.toEqual({
      status_code: 404,
      path: '/x',
    })
    expect(nextGave).toEqual({ path: '/x' })
    await expect(sdk.call({ function_id: 'http::on-request', payload: { path: '/boom' } })).rejects.toMatchObject({
      code: 'invocation_failed',
      message: 'invocation_failed: no route',
      function_id: 'http::on-request',
    })
  })

  it("next rejects with the handler's error, and with an InvocationError for a changed call that failed", async () => {
    const { sdk } = worker()
    const handlerError = new Error('card declined')
    const seen: unknown[] = []
    sdk
      .registerFunction('payments::charge', async () => {
        throw handlerError
      })
      .onObserverDeliver(async (call, observers, next) => {
        seen.push(await next(call, observers).catch((e) => e))
        seen.push(await next({ ...call, error: { code: 'DECLINED', message: 'declined' } }, observers).catch((e) => e))
        // not awaited: no unhandled rejection either way
        void next(call, observers)
        return 'handled'
      })
    fake.lastSocket().deliver(functionRegistered('payments::charge'))
    await settle()
    await expect(sdk.call({ function_id: 'payments::charge', payload: {} })).resolves.toBe('handled')
    expect(seen[0]).toBe(handlerError)
    expect(seen[1]).toBeInstanceOf(InvocationError)
    expect(seen[1]).toMatchObject({ code: 'DECLINED', function_id: 'payments::charge' })
  })

  it("runs inside the function's invocation: currentInvocation() and the caller of the calls it makes", async () => {
    const { sdk, socket } = worker()
    let inHook: InvocationContext | undefined
    let inHandler: InvocationContext | undefined
    sdk
      .registerFunction('orders::on-placed', (order: unknown) => {
        inHandler = sdk.currentInvocation()
        return order
      })
      .onObserverDeliver(async (call) => {
        await Promise.resolve()
        inHook = sdk.currentInvocation()
        void sdk.call({ function_id: 'audit::log', payload: {}, timeoutMs: 50 }).catch(() => undefined)
        return call.output
      })
    socket.deliver(functionRegistered('orders::on-placed'))
    await settle()
    socket.clear()
    socket.deliver(
      invokeFunction(
        'orders::on-placed',
        { id: 1 },
        {
          invocation_id: 'inv-routed',
          caller: { worker_id: 'w-shop', worker_name: 'shop', function_id: 'shop::checkout', invocation_id: 'inv-0' },
        },
      ),
    )
    await settle()
    expect(inHook).toEqual(inHandler)
    expect(inHook?.invocation_id).toBe('inv-routed')
    const [out] = socket.framesOf('invokefunction')
    expect(out.caller).toEqual({
      function_id: 'orders::on-placed',
      invocation_id: 'inv-routed',
      parent: { worker_id: 'w-shop', worker_name: 'shop', function_id: 'shop::checkout', invocation_id: 'inv-0' },
    })
  })

  it('delivers each observer at most once per call, however many times next runs', async () => {
    const { sdk, socket } = await cron([every('five', '*/5'), every('ten', '*/10')], (iii) =>
      iii.onObserverDeliver('cron::on-tick', async (call, observers, next) => {
        await next(call, observers)
        await next(call, observers)
        await next({ ...call, input: { ...call.input, n: 2 } }, observers)
        return call.output
      }),
    )
    await sdk.call({ function_id: 'cron::on-tick', payload: { expression: '*/5', n: 1 } })
    await flushed()
    expect(reportedItems(socket).map((i) => i.matched)).toEqual([['five', 'ten']])
  })

  it('local: a changed call is its own item; the unchanged calls pool into one item with the handler call', async () => {
    const { sdk, socket } = await cron(
      [
        every('all', '*', { capture: ['input', 'output'] }),
        every('two', '*', { filters: { $and: [{ 'input.n': { $eq: 2 } }] }, capture: ['input', 'output'] }),
      ],
      (iii) =>
        iii.onObserverDeliver<Tick, Tick>('cron::on-tick', async (call, observers, next) => {
          await next(call, observers)
          await next({ ...call, input: { ...call.input, n: 2 } }, observers)
          await next({ ...call }, observers) // a copy holding the same values is the same call
          return call.output as Tick
        }),
    )
    await sdk.call({ function_id: 'cron::on-tick', payload: { expression: '*', n: 1 } })
    await flushed()
    const items = reportedItems(socket)
    expect(items.map((i) => [i.matched, i.input, i.output, i.output_is_input])).toEqual([
      [['two'], { expression: '*', n: 2 }, { expression: '*', n: 1 }, undefined],
      [['all'], { expression: '*', n: 1 }, undefined, true],
    ])
    expect(new Set(items.map((i) => i.invocation_id)).size).toBe(1)
  })

  it('ignores what is not an observer of this function', async () => {
    const { sdk, socket } = await cron([every('five', '*/5')], (iii) =>
      iii.onObserverDeliver('cron::on-tick', (call, observers, next) =>
        next(call, [...observers, { id: 'forged', on: 'success', capture: ['input'] } as unknown as Observer]),
      ),
    )
    await sdk.call({ function_id: 'cron::on-tick', payload: { expression: '*/5', n: 1 } })
    await flushed()
    expect(reportedItems(socket).map((i) => i.matched)).toEqual([['five']])
  })
})

describe('routed calls: the host delivers (v2 §8.2)', () => {
  const answer = (socket: ReturnType<typeof fake.lastSocket>, id: string) => fake.resultOf(socket, id)

  type Order = { fail?: boolean; qty: number }
  async function routed(observers: Json[], hook?: DeliverHook<Order, { order_id: string; qty: number }, unknown>) {
    const { sdk, socket } = worker()
    const ref = sdk.registerFunction('orders::create', async (order: Order) => {
      if (order.fail) throw new Error('out of stock')
      return { order_id: 'o-1', qty: order.qty }
    })
    if (hook) ref.onObserverDeliver(hook)
    socket.deliver(functionRegistered('orders::create', observers))
    await settle()
    socket.clear()
    return { sdk, socket }
  }

  it("iii's delivery: the matched ids ride the answer, and nothing else is sent", async () => {
    const { socket } = await routed([
      observerInfo('big', 'alerts::big', { filters: { $and: [{ 'input.qty': { $gte: 10 } }] } }),
      observerInfo('all', 'audit::all'),
      observerInfo('errors', 'alerts::errors', { on: 'error' }),
    ])
    socket.deliver(invokeFunction('orders::create', { qty: 12 }, { invocation_id: 'i-1' }))
    socket.deliver(invokeFunction('orders::create', { qty: 1, fail: true }, { invocation_id: 'i-2' }))
    await settle()
    await flushed()
    expect(socket.frames().map((f) => f.type)).toEqual(['invocationresult', 'invocationresult'])
    expect(answer(socket, 'i-1')).toMatchObject({ result: { order_id: 'o-1', qty: 12 }, observed: ['big', 'all'] })
    // an error the handler threw: observed rides the error answer
    expect(answer(socket, 'i-2')).toMatchObject({
      error: { code: 'invocation_failed', message: 'out of stock' },
      observed: ['errors'],
    })
  })

  it('a call nobody matches is answered without observed', async () => {
    const { socket } = await routed([observerInfo('errors', 'alerts::errors', { on: 'error' })])
    socket.deliver(invokeFunction('orders::create', { qty: 1 }, { invocation_id: 'i-1' }))
    await settle()
    expect(socket.frames()).toEqual([
      {
        type: 'invocationresult',
        invocation_id: 'i-1',
        function_id: 'orders::create',
        result: { order_id: 'o-1', qty: 1 },
      },
    ])
  })

  it("an answer that is not the handler's own outcome: the pooled ids go in an item, sent before the answer", async () => {
    const all = observerInfo('all', 'audit::all', { on: 'complete', capture: ['input', 'output', 'error'] })
    const { socket } = await routed([all], async (call, obs, next) => {
      await next(call, obs).catch(() => undefined)
      return { recovered: true } // a handler error turned into a result
    })
    socket.deliver(invokeFunction('orders::create', { qty: 1, fail: true }, { invocation_id: 'i-1' }))
    await settle()
    const [batch, result] = socket.frames()
    expect(batch.type).toBe('invocationsobserved')
    expect(batch.items).toMatchObject([
      { invocation_id: 'i-1', matched: ['all'], status: 'error', error: { message: 'out of stock' } },
    ])
    expect(result).toMatchObject({ type: 'invocationresult', invocation_id: 'i-1', result: { recovered: true } })
    expect(result).not.toHaveProperty('observed')
  })

  it("a hook that rethrows the handler's error keeps observed; another error moves the ids to an item", async () => {
    const { socket } = await routed(
      [observerInfo('errors', 'alerts::errors', { on: 'error' })],
      async (call, obs, next) => {
        try {
          return await next(call, obs)
        } catch (error) {
          if ((call.input as { qty: number }).qty === 1) throw error
          throw new Error('wrapped')
        }
      },
    )
    socket.deliver(invokeFunction('orders::create', { qty: 1, fail: true }, { invocation_id: 'same' }))
    await settle()
    socket.deliver(invokeFunction('orders::create', { qty: 2, fail: true }, { invocation_id: 'other' }))
    await settle()
    expect(answer(socket, 'same')).toMatchObject({ error: { message: 'out of stock' }, observed: ['errors'] })
    expect(answer(socket, 'other')).toMatchObject({ error: { message: 'wrapped' } })
    expect(answer(socket, 'other')).not.toHaveProperty('observed')
    expect(reportedItems(socket).map((i) => [i.invocation_id, i.matched])).toEqual([['other', ['errors']]])
  })

  it('next given no observers delivers and reports nothing, whatever call it is given', async () => {
    const { socket } = await routed([], async (call, _obs, next) => {
      await next({ ...call, input: { qty: 99 } }, [])
      return call.output
    })
    socket.deliver(invokeFunction('orders::create', { qty: 1 }, { invocation_id: 'i-1' }))
    await settle()
    expect(socket.frames().map((f) => f.type)).toEqual(['invocationresult'])
  })

  it('a next call after the hook settled delivers nothing and is counted as LATE_DELIVERY', async () => {
    let later: (() => Promise<unknown>) | undefined
    const { socket } = await routed([observerInfo('all', 'audit::all')], (call, obs, next) => {
      later = () => next(call, obs)
      return call.output
    })
    socket.deliver(invokeFunction('orders::create', { qty: 1 }, { invocation_id: 'i-1' }))
    await settle()
    await later?.()
    await flushed()
    expect(reportedItems(socket)).toEqual([])
    expect(answer(socket, 'i-1')).not.toHaveProperty('observed')
    await fake.elapse(10_000)
    expect(socket.framesOf('localinvocationstats').flatMap((f) => f.dropped_reports as Json[])).toEqual([
      { function_id: 'orders::create', reason: 'LATE_DELIVERY', count: 1 },
    ])
  })

  it('items of a routed call whose connection closed before the hook settled count as DISCONNECTED', async () => {
    let release!: () => void
    const { socket } = await routed([observerInfo('all', 'audit::all')], async (call, obs, next) => {
      await next({ ...call, input: { qty: 7 } }, obs)
      await new Promise<void>((r) => {
        release = r
      })
      return call.output
    })
    socket.deliver(invokeFunction('orders::create', { qty: 1 }, { invocation_id: 'i-1' }))
    await settle()
    socket.drop()
    release()
    await settle()
    await fake.elapse(1500)
    const next = fake.lastSocket()
    next.open()
    await fake.elapse(10_000)
    expect(next.framesOf('invocationsobserved')).toEqual([])
    expect(next.framesOf('localinvocationstats').flatMap((f) => f.dropped_reports as Json[])).toEqual([
      { function_id: 'orders::create', reason: 'DISCONNECTED', count: 1 },
    ])
  })

  it('against an engine without observe.v2 the engine evaluates routed calls: no observed, no items', async () => {
    const { sdk, socket } = worker(fake.ENGINE_FEATURES_V1)
    sdk.registerFunction('orders::create', async () => ({ ok: true }))
    socket.deliver(functionRegistered('orders::create', [observerInfo('all', 'audit::all')]))
    await settle()
    socket.clear()
    socket.deliver(invokeFunction('orders::create', {}, { invocation_id: 'i-1' }))
    await settle()
    await flushed()
    expect(socket.frames()).toEqual([
      { type: 'invocationresult', invocation_id: 'i-1', function_id: 'orders::create', result: { ok: true } },
    ])
  })
})

describe('keyed delivery (v2 §4.5)', () => {
  const rowKey = (db: string, table: string, op: string) => `${db}|${table}|${op}`
  type Change = { db: string; table: string; op: string }
  const changes = keyed<Change, Change>({
    observer: ({ args }) =>
      ((args.ops as string[] | undefined) ?? ['*']).map((op) =>
        rowKey(args.db as string, (args.table as string | undefined) ?? '*', op),
      ),
    call: ({ input }) => [input.table, '*'].flatMap((t) => [input.op, '*'].map((op) => rowKey(input.db, t, op))),
  })

  async function database(observers: Json[]) {
    const { sdk, socket } = worker()
    const ref = sdk
      .registerFunction('database::on-row-changed', (change: Change) => change, { private: true })
      .onObserverDeliver(changes)
    socket.deliver(functionRegistered('database::on-row-changed', observers, { revision: 1 }))
    await settle()
    socket.clear()
    return { sdk, socket, ref }
  }

  const row = (id: string, args: Json, extra: { filters?: Json } = {}) =>
    observerInfo(id, `t::${id}`, { args, capture: ['input'], ...extra })

  it('a call reaches the observers filed under any of its keys, each once, through next (filters apply)', async () => {
    const { sdk, socket } = await database([
      row('orders-insert', { db: 'primary', table: 'orders', ops: ['insert'] }),
      row('orders-any', { db: 'primary', table: 'orders' }),
      row('every-insert', { db: 'primary', ops: ['insert', 'update'] }),
      row('big-only', { db: 'primary', table: 'orders' }, { filters: { $and: [{ 'input.big': { $eq: true } }] } }),
      row('users', { db: 'primary', table: 'users' }),
    ])
    await sdk.call({
      function_id: 'database::on-row-changed',
      payload: { db: 'primary', table: 'orders', op: 'insert' },
    })
    await flushed()
    expect(reportedItems(socket).map((i) => (i.matched as string[]).sort())).toEqual([
      ['every-insert', 'orders-any', 'orders-insert'],
    ])
  })

  it('isObserved(call): one lookup per call key; without a call, any active observer', async () => {
    const { sdk, ref } = await database([row('orders', { db: 'primary', table: 'orders', ops: ['insert'] })])
    expect(ref.isObserved()).toBe(true)
    expect(ref.isObserved({ input: { db: 'primary', table: 'orders', op: 'insert' } })).toBe(true)
    expect(ref.isObserved({ input: { db: 'primary', table: 'orders', op: 'delete' } })).toBe(false)
    expect(
      sdk.isObserved('database::on-row-changed', { input: { db: 'replica', table: 'orders', op: 'insert' } }),
    ).toBe(false)
    expect(sdk.isObserved('no::such-fn')).toBe(false)
  })

  it('files an observer when it is accepted and unfiles it when it is removed or rejected', async () => {
    const { socket, ref } = await database([])
    const orders = { input: { db: 'primary', table: 'orders', op: 'insert' } }
    expect(ref.isObserved(orders)).toBe(false)
    socket.deliver(observerAdded('database::on-row-changed', 2, row('a', { db: 'primary', table: 'orders' })))
    socket.deliver(observerAdded('database::on-row-changed', 3, row('b', { db: 'primary' })))
    await settle()
    expect(ref.isObserved(orders)).toBe(true)
    socket.deliver(observerRemoved('database::on-row-changed', 4, 'a'))
    await settle()
    expect(ref.isObserved(orders)).toBe(true)
    ref.observers()[0].reject('GONE', 'database dropped')
    expect(ref.isObserved(orders)).toBe(false)
    expect(ref.isObserved()).toBe(false)
  })

  it('a keyed hook set while observers are active files them; one whose keys throw is rejected', async () => {
    const { sdk, socket } = worker()
    const ref = sdk.registerFunction('cron::on-tick', (tick: Tick) => tick)
    socket.deliver(
      functionRegistered('cron::on-tick', [every('five', '*/5'), observerInfo('broken', 't::b', { args: {} })], {
        revision: 1,
      }),
    )
    await settle()
    ref.onObserverDeliver(
      keyed({
        observer: (obs) => {
          if (typeof obs.args.expression !== 'string') throw new Error('no expression')
          return obs.args.expression
        },
        call: (call) => call.input.expression,
      }),
    )
    expect(ref.observers().map((o) => o.id)).toEqual(['five'])
    expect(socket.framesOf('observerack').filter((a) => a.error)).toMatchObject([
      { id: 'broken', error: { code: 'OBSERVE_REJECTED' } },
    ])
    expect(ref.isObserved({ input: { expression: '*/5', n: 0 } })).toBe(true)
    // removing the hook: isObserved(call) no longer reads the call
    ref.onObserverDeliver(null)
    expect(ref.isObserved({ input: { expression: '*/9', n: 0 } })).toBe(true)
  })

  it('wrapped in another hook, keyed scans the observers it is given', async () => {
    const inner = keyed<Tick, Tick>({ observer: (o) => o.args.expression as string, call: (c) => c.input.expression })
    const { sdk, socket } = await cron([every('five', '*/5'), every('ten', '*/10')], (iii) =>
      iii.onObserverDeliver<Tick, Tick>('cron::on-tick', (call, observers, next) => inner(call, observers, next)),
    )
    await sdk.call({ function_id: 'cron::on-tick', payload: { expression: '*/5', n: 1 } })
    await flushed()
    expect(reportedItems(socket).map((i) => i.matched)).toEqual([['five']])
  })

  it('isObserved(call) on a function without keyed delivery ignores the call', async () => {
    const { sdk } = await cron([every('five', '*/5')], () => undefined)
    expect(sdk.isObserved('cron::on-tick', { input: { expression: 'anything' } })).toBe(true)
  })
})

describe('calling through an observer (v2 §4.6, decision 13)', () => {
  async function http() {
    const { sdk, socket } = worker()
    const routes = new Map<string, Observer>()
    sdk
      .registerFunction('http::on-request', (req: { path: string }) => req, { private: true })
      .onObserverAdded((obs) => {
        routes.set(obs.args.path as string, obs)
      })
      .onObserverRemoved((obs) => {
        routes.delete(obs.args.path as string)
      })
      .onObserverDeliver(async (call) => {
        const obs = routes.get(call.input.path)
        if (!obs) return { status_code: 404 }
        if (!obs.matches(call)) return { status_code: 422 }
        return sdk.call({ observer: obs, payload: call.input })
      })
    socket.deliver(
      functionRegistered('http::on-request', [
        observerInfo('users', 'users::get', {
          args: { method: 'GET', path: '/users' },
          filters: { $and: [{ 'input.tenant': { $ne: 'blocked' } }] },
        }),
      ]),
    )
    await settle()
    socket.clear()
    return { sdk, socket }
  }

  it("sends the observer's target with activation.observer_id and the hook's caller, no namespace; resolves to the target's result", async () => {
    const { sdk, socket } = await http()
    const response = sdk.call({ function_id: 'http::on-request', payload: { path: '/users', tenant: 'acme' } })
    await settle()
    const [frame] = socket.framesOf('invokefunction')
    expect(frame).toMatchObject({
      function_id: 'users::get',
      data: { path: '/users', tenant: 'acme' },
      activation: { observer_id: 'users' },
      caller: { function_id: 'http::on-request', invocation_id: expect.any(String) },
    })
    expect(frame).not.toHaveProperty('namespace')
    expect(frame).not.toHaveProperty('metadata')
    socket.deliver({
      type: 'invocationresult',
      invocation_id: frame.invocation_id,
      function_id: 'users::get',
      result: { status_code: 200, body: [] },
    })
    await expect(response).resolves.toEqual({ status_code: 200, body: [] })
  })

  it("the hook applies the observer's filters with obs.matches; nothing is sent for a miss", async () => {
    const { sdk, socket } = await http()
    await expect(
      sdk.call({ function_id: 'http::on-request', payload: { path: '/users', tenant: 'blocked' } }),
    ).resolves.toEqual({ status_code: 422 })
    await expect(sdk.call({ function_id: 'http::on-request', payload: { path: '/nope' } })).resolves.toEqual({
      status_code: 404,
    })
    expect(socket.framesOf('invokefunction')).toEqual([])
  })

  it("the engine's refusal reaches the caller as an InvocationError", async () => {
    const { sdk, socket } = await http()
    const response = sdk.call({ function_id: 'http::on-request', payload: { path: '/users' } })
    await settle()
    const [frame] = socket.framesOf('invokefunction')
    socket.deliver({
      type: 'invocationresult',
      invocation_id: frame.invocation_id,
      function_id: 'users::get',
      error: { code: 'OBSERVER_NOT_ACTIVE', message: 'observer users is not active' },
    })
    await expect(response).rejects.toMatchObject({ code: 'invocation_failed' })
  })

  it('accepted() resolves after the ack: a call through the observer sent then follows it on the wire', async () => {
    const { sdk, socket } = worker()
    sdk
      .registerFunction('inventory::on-stock', (e: unknown) => e)
      .onObserverAdded((obs) => {
        void obs
          .accepted()
          .then(() => sdk.call({ observer: obs, payload: { initial: true }, action: CallAction.Void() }))
      })
    socket.clear()
    socket.deliver(functionRegistered('inventory::on-stock'))
    socket.deliver(observerAdded('inventory::on-stock', 1, observerInfo('o', 'storefront::sync')))
    await settle()
    expect(
      socket
        .frames()
        .filter((f) => f.type === 'observerack' || f.type === 'invokefunction')
        .map((f) => f.type),
    ).toEqual(['observerack', 'invokefunction'])
    expect(socket.framesOf('invokefunction')[0]).toEqual({
      type: 'invokefunction',
      function_id: 'storefront::sync',
      data: { initial: true },
      action: { type: 'void' },
      activation: { observer_id: 'o' },
    })
  })

  it('accepted() rejects for an observer that is not accepted', async () => {
    const { sdk, socket } = worker()
    let obs: Observer | undefined
    sdk
      .registerFunction('inventory::on-stock', (e: unknown) => e)
      .onObserverAdded((o) => {
        obs = o
        throw new ObserveRejection('NO_STORE', 'unknown store')
      })
    socket.deliver(functionRegistered('inventory::on-stock', [observerInfo('o', 't::a')]))
    await settle()
    await expect(obs?.accepted()).rejects.toMatchObject({ name: 'ObserveRejection', code: 'NO_STORE' })
  })
})

describe('Observer.matches', () => {
  it("applies `on` against the call's status, then the filters; a throwing read does not match", async () => {
    const { sdk } = await cron(
      [
        every('ok', '*', { filters: { $and: [{ 'input.n': { $gte: 0 } }] } }),
        every('err', '*', { on: 'error' }),
        every('any', '*', { on: 'complete' }),
      ],
      () => undefined,
    )
    const [ok, err, any] = sdk.observers('cron::on-tick')
    const success: Partial<Call> = { status: 'success', input: { n: 1 } }
    const failure: Partial<Call> = { status: 'error', input: { n: 1 }, error: { code: 'x', message: 'y' } }
    expect([ok.matches(success), err.matches(success), any.matches(success)]).toEqual([true, false, true])
    expect([ok.matches(failure), err.matches(failure), any.matches(failure)]).toEqual([false, true, true])
    const hostile = {
      get n(): number {
        throw new Error('boom')
      },
    }
    expect(ok.matches({ input: hostile })).toBe(false)
  })
})
