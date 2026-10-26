// Derived from sdk/packages/node/iii/tests/observe-owner-delivery.test.ts by scripts/sync-browser-observe.py; edit those, not this file.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { registerWorker } from '../src/index'
import type { Observer } from '../src/observe-types'
import type { FunctionRef, ISdk } from '../src/types'
import * as fake from './observe-fake-engine'

/** Owner delivery against a fake engine: `obs.emit` / `obs.call` and their caller rules (sdk.md §1, §4.3). */
const { functionRegistered, observerInfo, observerRemoved, settle, reportedItems } = fake

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

type Tick = { expression: string; n: number }

/**
 * A cron-like owner: `cron::on-tick` indexes its observers by expression and
 * emits each tick to that expression's observers.
 */
async function cronOwner(observers: Record<string, unknown>[]) {
  sdk = registerWorker('ws://owner.test', {
    workerName: 'cron',
  })
  const socket = fake.lastSocket()
  const index = new Map<string, Set<Observer>>()
  const emitted: boolean[] = []
  const iii = sdk
  const ref: FunctionRef = iii.registerFunction(
    'cron::on-tick',
    async (tick: Tick) => {
      for (const obs of index.get(tick.expression) ?? []) emitted.push(obs.emit(tick))
      return null
    },
    {
      observe: {
        delivery: 'owner',
        args_format: { type: 'object', required: ['expression'] },
        onObserverAdded: (obs) => {
          const expression = obs.args.expression as string
          if (!index.has(expression)) index.set(expression, new Set())
          index.get(expression)?.add(obs)
        },
        onObserverRemoved: (obs) => {
          index.get(obs.args.expression as string)?.delete(obs)
        },
      },
    },
  )
  iii.registerFunction('billing::close', async (tick: Tick) => {
    for (const obs of index.get(tick.expression) ?? []) emitted.push(obs.emit(tick))
    return null
  })
  socket.open()
  socket.deliver(functionRegistered('cron::on-tick', observers, { revision: 1 }))
  socket.deliver(functionRegistered('billing::close'))
  await settle()
  socket.clear()
  return { sdk: iii, socket, ref, index, emitted }
}

const every5 = (id: string, extra: { filters?: Record<string, unknown>; capture?: string[] } = {}) =>
  observerInfo(id, `local::${id}`, { args: { expression: '*/5' }, capture: ['input', 'caller'], ...extra })

describe('obs.emit', () => {
  it('returns false and sends nothing when the filters reject the event', async () => {
    const { sdk, socket, emitted } = await cronOwner([
      every5('late', { filters: { $and: [{ 'input.n': { $gte: 10 } }] } }),
    ])
    await sdk.trigger({ function_id: 'cron::on-tick', payload: { expression: '*/5', n: 1 } })
    await flushed()
    expect(emitted).toEqual([false])
    expect(socket.sent).toHaveLength(0)
  })

  it('reaches only the observers the owner routes to (two observers with different args)', async () => {
    const { sdk, socket } = await cronOwner([
      every5('five'),
      observerInfo('ten', 'local::ten', { args: { expression: '*/10' }, capture: ['input'] }),
    ])
    await sdk.trigger({ function_id: 'cron::on-tick', payload: { expression: '*/10', n: 1 } })
    await flushed()
    const items = reportedItems(socket)
    expect(items.map((i) => i.matched)).toEqual([['ten']])
    expect(items[0].input).toEqual({ expression: '*/10', n: 1 })
  })

  it('inside the observed function’s own handler, the event is that invocation (its id, its caller)', async () => {
    const { sdk, socket } = await cronOwner([every5('five')])
    socket.deliver({
      type: 'invokefunction',
      invocation_id: 'inv-remote',
      function_id: 'cron::on-tick',
      data: { expression: '*/5', n: 2 },
      caller: { worker_id: 'w-sched', worker_name: 'scheduler', function_id: 'sched::fire', invocation_id: 'inv-0' },
      observe_depth: 1,
      metadata: { run: 7 },
    })
    await settle()
    await flushed()
    const [item] = reportedItems(socket)
    expect(item.invocation_id).toBe('inv-remote')
    expect(item).toMatchObject({
      function_id: 'cron::on-tick',
      matched: ['five'],
      status: 'success',
      caller: { worker_id: 'w-sched', worker_name: 'scheduler', function_id: 'sched::fire', invocation_id: 'inv-0' },
      observe_depth: 1,
      duration_ms: 0,
    })
    expect(item).not.toHaveProperty('metadata') // not captured
    void sdk
  })

  it('inside another handler, that handler is the caller', async () => {
    const { sdk, socket } = await cronOwner([every5('five')])
    await sdk.trigger({ function_id: 'billing::close', payload: { expression: '*/5', n: 3 } })
    await flushed()
    const [item] = reportedItems(socket)
    expect(item.caller).toEqual({
      worker_id: 'worker-1',
      worker_name: 'cron',
      function_id: 'billing::close',
      invocation_id: expect.any(String),
      parent: { worker_id: 'worker-1', worker_name: 'cron' },
    })
  })

  it('an event whose reading throws does not match, the same in emit, call and matches', async () => {
    const { socket, index } = await cronOwner([every5('five', { filters: { $and: [{ 'input.n': { $gte: 0 } }] } })])
    const [obs] = [...(index.get('*/5') ?? [])]
    const hostile = {
      expression: '*/5',
      get n(): number {
        throw new Error('boom')
      },
    }
    expect(obs.matches({ input: hostile })).toBe(false)
    expect(obs.emit(hostile)).toBe(false)
    await expect(obs.call(hostile)).rejects.toMatchObject({ code: 'FILTERS_NOT_MATCHED' })
    await flushed()
    expect(socket.sent).toHaveLength(0)
  })

  it('outside any handler, this worker only', async () => {
    const { socket, index } = await cronOwner([every5('five')])
    const [obs] = [...(index.get('*/5') ?? [])]
    expect(obs.emit({ expression: '*/5', n: 4 })).toBe(true)
    await flushed()
    const [item] = reportedItems(socket)
    expect(item.caller).toEqual({ worker_id: 'worker-1', worker_name: 'cron' })
    expect(typeof item.invocation_id).toBe('string')
  })

  it('emits of one event to N observers in one flush are one report item', async () => {
    const { sdk, socket } = await cronOwner([
      every5('a'),
      every5('b', { capture: ['input'] }),
      every5('c', { filters: { $and: [{ 'input.n': { $lt: 0 } }] } }),
      every5('d', { capture: ['caller'] }),
    ])
    await sdk.trigger({ function_id: 'cron::on-tick', payload: { expression: '*/5', n: 5 } })
    await sdk.trigger({ function_id: 'cron::on-tick', payload: { expression: '*/5', n: 6 } })
    await flushed()
    const frames = socket.framesOf('invocationsobserved')
    expect(frames).toHaveLength(1)
    const items = reportedItems(socket)
    expect(items.map((i) => [i.matched, (i.input as Tick).n])).toEqual([
      [['a', 'b', 'd'], 5],
      [['a', 'b', 'd'], 6],
    ])
    expect(items[0]).toHaveProperty('caller')
  })

  it('throws NOT_OWNER_DELIVERED on an auto function; once removed, emit is false and call is OBSERVER_NOT_ACTIVE', async () => {
    const { socket, index } = await cronOwner([every5('five')])
    const [obs] = [...(index.get('*/5') ?? [])]

    let autoObs: Observer | undefined
    sdk?.registerFunction('auto::fn', async () => null, {
      observe: {
        onObserverAdded: (o) => {
          autoObs = o
        },
      },
    })
    socket.deliver(functionRegistered('auto::fn', [observerInfo('x', 't::x')]))
    await settle()
    expect(() => autoObs?.emit({})).toThrow(/NOT_OWNER_DELIVERED/)
    await expect(autoObs?.call({})).rejects.toMatchObject({ code: 'NOT_OWNER_DELIVERED' })

    socket.deliver(observerRemoved('cron::on-tick', 2, 'five'))
    await settle()
    socket.clear()
    expect(obs.emit({ expression: '*/5', n: 1 })).toBe(false) // an owner's fan-out loop goes on
    await expect(obs.call({})).rejects.toMatchObject({ code: 'OBSERVER_NOT_ACTIVE' })
    await flushed()
    expect(socket.sent).toHaveLength(0)
  })

  it('obs.reject sends a late rejection and stops delivering to it', async () => {
    const { socket, index } = await cronOwner([every5('five')])
    const [obs] = [...(index.get('*/5') ?? [])]
    obs.reject('UNKNOWN_DB', 'the database was removed from the config')
    expect(socket.framesOf('observerack')).toEqual([
      {
        type: 'observerack',
        function_id: 'cron::on-tick',
        namespace: 'default',
        id: 'five',
        revision: 1,
        error: { code: 'UNKNOWN_DB', message: 'the database was removed from the config' },
      },
    ])
    expect(obs.emit({ expression: '*/5', n: 1 })).toBe(false)
  })

  it('obs.reject sends the revision the engine last announced for the observer', async () => {
    const { socket, index } = await cronOwner([every5('five')])
    socket.deliver(functionRegistered('cron::on-tick', [every5('five')], { revision: 7 })) // re-acked at 7
    await settle()
    const [obs] = [...(index.get('*/5') ?? [])]
    obs.reject('UNKNOWN_DB', 'gone')
    expect(socket.framesOf('observerack').map((a) => [a.revision, a.error])).toEqual([
      [7, undefined],
      [7, { code: 'UNKNOWN_DB', message: 'gone' }],
    ])
  })

  it('a late rejection made while disconnected survives the reconnect; a fresh observeradded reactivates', async () => {
    const { sdk, socket, index, ref } = await cronOwner([every5('five')])
    const [obs] = [...(index.get('*/5') ?? [])]
    socket.drop()
    obs.reject('UNKNOWN_DB', 'gone from the config') // the ack is lost with the socket
    expect(ref.isObserved()).toBe(false)

    await fake.elapse(1500) // the reconnect
    const next = fake.lastSocket()
    expect(next).not.toBe(socket)
    next.open()
    next.deliver(functionRegistered('cron::on-tick', [every5('five')], { revision: 1 }))
    await settle()
    expect(next.framesOf('observerack')).toEqual([
      {
        type: 'observerack',
        function_id: 'cron::on-tick',
        namespace: 'default',
        id: 'five',
        revision: 1,
        error: { code: 'UNKNOWN_DB', message: 'gone from the config' },
      },
    ])
    expect(ref.isObserved()).toBe(false)
    expect(obs.emit({ expression: '*/5', n: 1 })).toBe(false)

    // the engine adds it again: removed, then added afresh (hooks run), active
    next.deliver(fake.observerAdded('cron::on-tick', 2, every5('five')))
    await settle()
    expect(ref.isObserved()).toBe(true)
    expect(next.framesOf('observerack').slice(-1)[0]).toEqual({
      type: 'observerack',
      function_id: 'cron::on-tick',
      namespace: 'default',
      id: 'five',
      revision: 2,
    })
    const [fresh] = [...(index.get('*/5') ?? [])]
    expect(fresh).not.toBe(obs)
    await sdk.trigger({ function_id: 'cron::on-tick', payload: { expression: '*/5', n: 2 } })
    await flushed()
    expect(reportedItems(next).map((i) => i.matched)).toEqual([['five']])
  })

  it('emitted items carry the trace context of the invocation they were emitted in', async () => {
    const { socket } = await cronOwner([every5('five')])
    const traceparent = '00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01'
    socket.deliver({
      type: 'invokefunction',
      invocation_id: 'inv-traced',
      function_id: 'cron::on-tick',
      data: { expression: '*/5', n: 1 },
      traceparent,
      baggage: 'tenant=acme',
    })
    await settle()
    await flushed()
    const [item] = reportedItems(socket)
    expect(item).toMatchObject({ invocation_id: 'inv-traced', traceparent, baggage: 'tenant=acme' })
  })
})

describe('obs.call', () => {
  async function httpOwner(filters: Record<string, unknown> = {}) {
    sdk = registerWorker('ws://owner.test', {
      workerName: 'http',
      namespace: 'acme',
    })
    const socket = fake.lastSocket()
    const routes = new Map<string, Observer>()
    const iii = sdk
    iii.registerFunction(
      'http::on-request',
      async (req: { path: string; tenant: string }) => {
        const obs = routes.get(req.path)
        if (!obs) return { status: 404 }
        return obs.call(req, { timeoutMs: 200 })
      },
      {
        observe: {
          delivery: 'owner',
          args_format: { type: 'object' },
          onObserverAdded: (obs) => {
            routes.set(obs.args.path as string, obs)
          },
        },
      },
    )
    socket.open()
    socket.deliver(
      functionRegistered(
        'http::on-request',
        [observerInfo('route-1', 'users::get', { args: { path: '/u' }, filters })],
        {
          namespace: 'acme',
        },
      ),
    )
    await settle()
    socket.clear()
    return { sdk: iii, socket, routes }
  }

  it("invokes the target through the engine with the activation, and resolves with the target's result", async () => {
    const { sdk, socket } = await httpOwner()
    const response = sdk.trigger({ function_id: 'http::on-request', payload: { path: '/u', tenant: 'acme' } })
    await settle()
    const [call] = socket.framesOf('invokefunction')
    const { traceparent: _, baggage: __, ...rest } = call
    expect(rest).toEqual({
      type: 'invokefunction',
      invocation_id: expect.any(String),
      function_id: 'users::get',
      data: { path: '/u', tenant: 'acme' },
      caller: { function_id: 'http::on-request', invocation_id: expect.any(String), parent: expect.any(Object) },
      activation: { observer_id: 'route-1' },
    })
    expect(call).not.toHaveProperty('namespace') // CONTRACT D13
    expect(call).not.toHaveProperty('metadata')
    socket.deliver({
      type: 'invocationresult',
      invocation_id: call.invocation_id,
      function_id: 'users::get',
      result: { status: 200, body: { id: 42 } },
    })
    await expect(response).resolves.toEqual({ status: 200, body: { id: 42 } })
  })

  it('rejects with FILTERS_NOT_MATCHED and sends nothing when the filters reject the payload', async () => {
    const { sdk, socket, routes } = await httpOwner({ $and: [{ 'input.tenant': { $eq: 'acme' } }] })
    await expect(
      sdk.trigger({ function_id: 'http::on-request', payload: { path: '/u', tenant: 'other' } }),
    ).rejects.toMatchObject({ message: expect.stringContaining('FILTERS_NOT_MATCHED') })
    await expect(routes.get('/u')?.call({ path: '/u', tenant: 'other' })).rejects.toMatchObject({
      code: 'FILTERS_NOT_MATCHED',
    })
    expect(socket.sent).toHaveLength(0)
    expect(routes.get('/u')?.matches({ input: { tenant: 'acme' } })).toBe(true)
  })

  it('surfaces the engine error code of a refused owner delivery', async () => {
    const { socket, routes } = await httpOwner()
    const result = routes.get('/u')?.call({ path: '/u' })
    await settle()
    const [call] = socket.framesOf('invokefunction')
    socket.deliver({
      type: 'invocationresult',
      invocation_id: call.invocation_id,
      function_id: 'users::get',
      error: { code: 'OBSERVER_NOT_ACTIVE', message: 'observer route-1 is pending' },
    })
    await expect(result).rejects.toMatchObject({ code: 'OBSERVER_NOT_ACTIVE' })
  })
})

describe('FunctionRef.isObserved() / observers()', () => {
  it('reflect the active observers', async () => {
    const { ref, socket } = await cronOwner([every5('a'), every5('b')])
    expect(ref.isObserved()).toBe(true)
    expect(ref.observers().map((o) => o.id)).toEqual(['a', 'b'])
    socket.deliver(observerRemoved('cron::on-tick', 2, 'a'))
    socket.deliver(observerRemoved('cron::on-tick', 3, 'b'))
    await settle()
    expect(ref.isObserved()).toBe(false)
    expect(ref.observers()).toEqual([])
  })
})
