// Derived from sdk/packages/node/iii/tests/observe-deliver.test.ts by scripts/sync-browser-observe.py; edit those, not this file.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ObserveRejection } from '../src/errors'
import { CallAction, registerWorker } from '../src/index'
import type { Call, InvocationContext, MatchKeys, MatchPredicate, Observer } from '../src/observe-types'
import type { FunctionRef, ISdk } from '../src/types'
import * as fake from './observe-fake-engine'

/**
 * Matching and delivery in the host (v2 §4.4–§4.6, §8.2; decisions 13, 20,
 * 36, 38, 39, 45): onObserverMatch (predicate and keys), onObserverDeliver,
 * isObserved(call), routed calls and calls through an observer, against a
 * fake engine.
 */
const { functionRegistered, observerInfo, observerAdded, observerRemoved, settle, reportedItems, invokeFunction } = fake

type Json = Record<string, unknown>

let sdk: ISdk | undefined
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
async function cron(observers: Json[], hooks: (iii: ISdk) => void) {
  const { sdk, socket } = worker()
  sdk.registerFunction('cron::on-tick', (tick: Tick) => tick, { private: true })
  hooks(sdk)
  socket.deliver(functionRegistered('cron::on-tick', observers, { revision: 1 }))
  await settle()
  socket.clear()
  return { sdk, socket }
}

const byExpression: MatchPredicate<Tick, Tick> = (obs, call) => obs.args.expression === call.input.expression

describe('onObserverMatch: a predicate', () => {
  it('only the observers that want the call are delivered; their filters still apply', async () => {
    const { sdk, socket } = await cron(
      [
        every('five', '*/5'),
        every('ten', '*/10'),
        every('ten-late', '*/10', { filters: { $and: [{ 'input.n': { $gte: 10 } }] } }),
      ],
      (iii) => iii.onObserverMatch<Tick, Tick>('cron::on-tick', byExpression),
    )
    await expect(sdk.call({ function_id: 'cron::on-tick', payload: { expression: '*/10', n: 1 } })).resolves.toEqual({
      expression: '*/10',
      n: 1,
    })
    await flushed()
    expect(reportedItems(socket).map((i) => [i.matched, i.input])).toEqual([[['ten'], { expression: '*/10', n: 1 }]])
  })

  it('a predicate that throws matches nothing, and is logged once per function', async () => {
    const errors = vi.spyOn(console, 'error').mockImplementation(() => undefined)
    const { sdk, socket } = await cron([every('five', '*/5'), every('ten', '*/10')], (iii) =>
      iii.onObserverMatch<Tick, Tick>('cron::on-tick', (obs, call) => {
        if (obs.id === 'ten') throw new Error('bad observer')
        return byExpression(obs, call)
      }),
    )
    for (const expression of ['*/5', '*/10', '*/10']) {
      await sdk.call({ function_id: 'cron::on-tick', payload: { expression, n: 1 } })
    }
    await flushed()
    expect(reportedItems(socket).map((i) => i.matched)).toEqual([['five']])
    expect(errors.mock.calls.filter((c) => String(c[0]).includes('onObserverMatch of cron::on-tick'))).toHaveLength(1)
  })

  it('isObserved(call) does not read the call; without a match hook every observer wants every call', async () => {
    const { sdk, socket } = await cron([every('five', '*/5')], (iii) =>
      iii.onObserverMatch<Tick, Tick>('cron::on-tick', byExpression),
    )
    expect(sdk.isObserved('cron::on-tick', { input: { expression: '*/9', n: 0 } })).toBe(true)
    sdk.onObserverMatch('cron::on-tick', null)
    await sdk.call({ function_id: 'cron::on-tick', payload: { expression: '*/9', n: 0 } })
    await flushed()
    expect(reportedItems(socket).map((i) => i.matched)).toEqual([['five']])
  })

  it('refuses a match hook that is neither a predicate, key functions nor null', () => {
    const { sdk } = worker()
    expect(() => sdk.onObserverMatch('cron::on-tick', { observer: () => 'k' } as never)).toThrow(TypeError)
    expect(() => sdk.onObserverMatch('cron::on-tick', 'expression' as never)).toThrow(TypeError)
  })
})

describe('onObserverMatch: keys (v2 §4.5)', () => {
  const rowKey = (db: string, table: string, op: string) => `${db}|${table}|${op}`
  type Change = { db: string; table: string; op: string; big?: boolean }
  const byRow: MatchKeys<Change, Change> = {
    observer: ({ args }) =>
      ((args.ops as string[] | undefined) ?? ['*']).map((op) =>
        rowKey(args.db as string, (args.table as string | undefined) ?? '*', op),
      ),
    call: ({ input }) => [input.table, '*'].flatMap((t) => [input.op, '*'].map((op) => rowKey(input.db, t, op))),
  }

  async function database(observers: Json[]) {
    const { sdk, socket } = worker()
    const ref = sdk
      .registerFunction('database::on-row-changed', (change: Change) => change, { private: true })
      .onObserverMatch(byRow)
    socket.deliver(functionRegistered('database::on-row-changed', observers, { revision: 1 }))
    await settle()
    socket.clear()
    return { sdk, socket, ref }
  }

  const row = (id: string, args: Json, extra: { filters?: Json } = {}) =>
    observerInfo(id, `t::${id}`, { args, capture: ['input'], ...extra })

  it('a call matches the observers filed under any of its keys, each once; filters still apply', async () => {
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

  it('an observer whose keys throw when it is added is rejected (OBSERVE_REJECTED)', async () => {
    const { socket, ref } = await database([])
    socket.deliver(observerAdded('database::on-row-changed', 2, row('bad-ops', { db: 'primary', ops: 5 })))
    await settle()
    expect(socket.framesOf('observerack')).toMatchObject([{ id: 'bad-ops', error: { code: 'OBSERVE_REJECTED' } }])
    expect(ref.observers()).toEqual([])
  })

  it('set while observers are active, it builds the index from them; one whose keys throw is rejected', async () => {
    const { sdk, socket } = worker()
    const ref = sdk.registerFunction('cron::on-tick', (tick: Tick) => tick)
    socket.deliver(
      functionRegistered('cron::on-tick', [every('five', '*/5'), observerInfo('broken', 't::b', { args: {} })], {
        revision: 1,
      }),
    )
    await settle()
    ref.onObserverMatch({
      observer: (obs) => {
        if (typeof obs.args.expression !== 'string') throw new Error('no expression')
        return obs.args.expression
      },
      call: (call) => call.input.expression,
    })
    expect(ref.observers().map((o) => o.id)).toEqual(['five'])
    expect(socket.framesOf('observerack').filter((a) => a.error)).toMatchObject([
      { id: 'broken', error: { code: 'OBSERVE_REJECTED' } },
    ])
    expect(ref.isObserved({ input: { expression: '*/5', n: 0 } })).toBe(true)
    expect(ref.isObserved({ input: { expression: '*/9', n: 0 } })).toBe(false)
    // a predicate in its place: isObserved(call) no longer reads the call
    ref.onObserverMatch(byExpression)
    expect(ref.isObserved({ input: { expression: '*/9', n: 0 } })).toBe(true)
  })

  it('a call matches the observers active when it started: one filed under its key while it runs gets nothing', async () => {
    const { sdk, socket } = worker()
    let release: () => void = () => undefined
    const ref = sdk
      .registerFunction('database::on-row-changed', async (change: Change) => {
        await new Promise<void>((resolve) => {
          release = resolve
        })
        return change
      })
      .onObserverMatch(byRow)
    socket.deliver(
      functionRegistered('database::on-row-changed', [row('before', { db: 'primary', table: 'orders' })], {
        revision: 1,
      }),
    )
    await settle()
    socket.clear()
    const orders = { db: 'primary', table: 'orders', op: 'insert' }
    const during = async (revision: number, id: string) => {
      const call = sdk.call({ function_id: 'database::on-row-changed', payload: orders })
      await settle()
      socket.deliver(observerAdded('database::on-row-changed', revision, row(id, { db: 'primary', table: 'orders' })))
      await settle()
      expect(ref.observers().map((o) => o.id)).toContain(id)
      release()
      await call
    }

    await during(2, 'added-during-call')
    await flushed()
    expect(reportedItems(socket).map((i) => i.matched)).toEqual([['before']])

    const delivered: string[][] = []
    ref.onObserverDeliver((call, observers) => {
      delivered.push(observers.map((o) => o.id).sort())
      return call.output
    })
    await during(3, 'added-during-hooked-call')
    expect(delivered).toEqual([['added-during-call', 'before']])
  })

  it('call keys that throw match nothing', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined)
    const { sdk, socket } = await database([row('any', { db: 'primary' })])
    await sdk.call({ function_id: 'database::on-row-changed', payload: { db: 'primary', table: 'x', op: 'insert' } })
    sdk.onObserverMatch('database::on-row-changed', {
      observer: () => 'k',
      call: () => {
        throw new Error('no key')
      },
    })
    await sdk.call({ function_id: 'database::on-row-changed', payload: { db: 'primary', table: 'x', op: 'insert' } })
    expect(sdk.isObserved('database::on-row-changed', { input: {} })).toBe(false)
    await flushed()
    expect(reportedItems(socket)).toHaveLength(1) // the first call, matched by byRow's keys
  })
})

describe('onObserverDeliver', () => {
  it('what the hook returns or throws is what the caller gets back; iii delivers nothing for the function', async () => {
    const { sdk, socket } = worker()
    let seen: readonly Observer[] = []
    sdk
      .registerFunction('http::on-request', (req: { path: string }) => req)
      .onObserverDeliver(async (call, observers) => {
        seen = observers
        if (call.input.path === '/boom') throw new Error('no route')
        return { status_code: 404, path: call.input.path }
      })
    socket.deliver(functionRegistered('http::on-request', [observerInfo('route', 'users::get')]))
    await settle()
    socket.clear()
    await expect(sdk.call({ function_id: 'http::on-request', payload: { path: '/x' } })).resolves.toEqual({
      status_code: 404,
      path: '/x',
    })
    expect(seen.map((o) => o.id)).toEqual(['route'])
    // The browser SDK rejects with the error body the remote path gives.
    await expect(sdk.call({ function_id: 'http::on-request', payload: { path: '/boom' } })).rejects.toEqual({
      code: 'invocation_failed',
      message: 'no route',
      stacktrace: expect.any(String),
    })
    await flushed()
    expect(socket.sent).toHaveLength(0)
  })

  it('an ObserveRejection the hook throws reaches the caller with its code and message: local and routed', async () => {
    const { sdk, socket } = worker()
    sdk
      .registerFunction('queue::on-message', (message: { n: number }) => message)
      .onObserverMatch({ observer: (obs) => obs.id, call: (call) => (call.metadata?.subscriber as string) ?? '' })
      .onObserverDeliver(async (call, [obs]) => {
        if (!obs) throw new ObserveRejection('OBSERVER_NOT_ACTIVE', 'subscriber removed')
        return call.output
      })
    socket.deliver(functionRegistered('queue::on-message', [observerInfo('sub-1', 'billing::on-message')]))
    await settle()
    socket.clear()
    await expect(
      sdk.call({ function_id: 'queue::on-message', payload: { n: 1 }, metadata: { subscriber: 'sub-1' } }),
    ).resolves.toEqual({ n: 1 })
    await expect(
      sdk.call({ function_id: 'queue::on-message', payload: { n: 2 }, metadata: { subscriber: 'sub-2' } }),
    ).rejects.toEqual({ code: 'OBSERVER_NOT_ACTIVE', message: 'subscriber removed' }) // the remote path's error body
    socket.deliver(
      invokeFunction('queue::on-message', { n: 3 }, { invocation_id: 'routed', metadata: { subscriber: 'sub-2' } }),
    )
    await settle()
    expect(fake.resultOf(socket, 'routed')).toEqual({
      type: 'invocationresult',
      invocation_id: 'routed',
      function_id: 'queue::on-message',
      error: { code: 'OBSERVER_NOT_ACTIVE', message: 'subscriber removed' },
    })
  })

  it('runs with no observers too, and receives only the observers that matched', async () => {
    const seen: string[][] = []
    const { sdk } = await cron([every('five', '*/5'), every('ten', '*/10')], (iii) =>
      iii
        .onObserverMatch<Tick, Tick>('cron::on-tick', byExpression)
        .onObserverDeliver<Tick, Tick, string>('cron::on-tick', (call, observers) => {
          seen.push(observers.map((o) => o.id))
          return `delivered ${call.input.expression}`
        }),
    )
    await expect(sdk.call({ function_id: 'cron::on-tick', payload: { expression: '*/10', n: 1 } })).resolves.toBe(
      'delivered */10',
    )
    await sdk.call({ function_id: 'cron::on-tick', payload: { expression: '* * *', n: 2 } })
    expect(seen).toEqual([['ten'], []])
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
        // Browser SDK: the scope ends at the hook's first `await`. A call made
        // after it still has the invocation as its caller: the only one running,
        // and delivering.
        inHook = sdk.currentInvocation()
        await Promise.resolve()
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
})

describe('routed calls: the host delivers (v2 §8.2)', () => {
  const answer = (socket: ReturnType<typeof fake.lastSocket>, id: string) => fake.resultOf(socket, id)

  type Order = { fail?: boolean; qty: number }
  type Created = { order_id: string; qty: number }
  async function routed(observers: Json[], hooks: (ref: FunctionRef<Order, Created>) => void = () => undefined) {
    const { sdk, socket } = worker()
    const ref = sdk.registerFunction('orders::create', async (order: Order) => {
      if (order.fail) throw new Error('out of stock')
      return { order_id: 'o-1', qty: order.qty }
    })
    hooks(ref)
    socket.deliver(functionRegistered('orders::create', observers))
    await settle()
    socket.clear()
    return { sdk, socket }
  }

  it("iii's delivery: the selected ids ride the answer, on success and on the handler's error; no report is sent", async () => {
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

  it('the match hook applies to routed calls', async () => {
    const { socket } = await routed(
      [
        observerInfo('small', 'audit::small', { args: { max: 5 } }),
        observerInfo('large', 'audit::large', { args: { max: 50 } }),
      ],
      (ref) => ref.onObserverMatch((obs, call) => call.input.qty <= (obs.args.max as number)),
    )
    socket.deliver(invokeFunction('orders::create', { qty: 10 }, { invocation_id: 'i-1' }))
    await settle()
    expect(answer(socket, 'i-1')).toMatchObject({ observed: ['large'] })
  })

  it('a function with onObserverDeliver lists no observed: its answer is what the hook returns or throws', async () => {
    const { socket } = await routed([observerInfo('all', 'audit::all', { on: 'complete' })], (ref) =>
      ref.onObserverDeliver(async (call) => {
        if (call.status === 'error') return { recovered: true }
        throw new Error('rejected by the hook')
      }),
    )
    socket.deliver(invokeFunction('orders::create', { qty: 1, fail: true }, { invocation_id: 'recovered' }))
    socket.deliver(invokeFunction('orders::create', { qty: 1 }, { invocation_id: 'rejected' }))
    await settle()
    await flushed()
    expect(socket.frames().map((f) => f.type)).toEqual(['invocationresult', 'invocationresult'])
    expect(answer(socket, 'recovered')).toEqual({
      type: 'invocationresult',
      invocation_id: 'recovered',
      function_id: 'orders::create',
      result: { recovered: true },
    })
    expect(answer(socket, 'rejected')).toMatchObject({
      error: { code: 'invocation_failed', message: 'rejected by the hook' },
    })
    expect(answer(socket, 'rejected')).not.toHaveProperty('observed')
  })

  it('against an engine without observe.v2 the engine evaluates routed calls: no observed', async () => {
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

  it("the engine's refusal, rethrown by the hook, reaches the caller with its code: local and routed", async () => {
    const { sdk, socket } = await http()
    const refuse = (frame: Json) =>
      socket.deliver({
        type: 'invocationresult',
        invocation_id: frame.invocation_id,
        function_id: 'users::get',
        error: { code: 'OBSERVER_NOT_ACTIVE', message: 'observer users is not active' },
      })
    const response = sdk.call({ function_id: 'http::on-request', payload: { path: '/users' } })
    await settle()
    refuse(socket.framesOf('invokefunction')[0])
    // The browser SDK rejects with the error body the remote path gives.
    await expect(response).rejects.toEqual({ code: 'OBSERVER_NOT_ACTIVE', message: 'observer users is not active' })
    socket.clear()
    socket.deliver(invokeFunction('http::on-request', { path: '/users' }, { invocation_id: 'routed' }))
    await settle()
    refuse(socket.framesOf('invokefunction')[0])
    await settle()
    expect(fake.resultOf(socket, 'routed')).toEqual({
      type: 'invocationresult',
      invocation_id: 'routed',
      function_id: 'http::on-request',
      error: { code: 'OBSERVER_NOT_ACTIVE', message: 'observer users is not active' },
    })
  })

  it('a call through the observer that times out, rethrown by the hook, reaches the caller as TIMEOUT', async () => {
    const { sdk } = await http()
    sdk.onObserverDeliver('http::on-request', async (call, [obs]) =>
      sdk.call({ observer: obs, payload: call.input, timeoutMs: 20 }),
    )
    const response = sdk.call({ function_id: 'http::on-request', payload: { path: '/users' } })
    const outcome = expect(response).rejects.toMatchObject({ code: 'TIMEOUT' })
    await fake.elapse(25)
    await outcome
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
