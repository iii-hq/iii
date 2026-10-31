import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Call, InvocationContext, MatchKeys, MatchPredicate, Observer } from '../src/observe-types'
import type { FunctionRef, IIIClient } from '../src/types'

/**
 * Matching and delivery in the host (v2 §4.4–§4.6, §8.2; decisions 13, 20,
 * 36, 38, 39, 45, 55): onObserverMatch (predicate and keys),
 * onObserverDeliver, isObserved(call), the answers to routed calls and calls
 * through an observer, against a fake engine.
 */
vi.mock('ws', async () => (await import('./observe-fake-engine')).wsModule())
vi.resetModules()
const fake = await import('./observe-fake-engine')
const { registerWorker, CallAction } = await import('../src/index')
const { ObserveRejection } = await import('../src/errors')
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
async function cron(observers: Json[], hooks: (iii: IIIClient) => void) {
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
      // filed under two of the call's keys: primary|orders|insert and primary|orders|*
      row('orders-insert-or-any', { db: 'primary', table: 'orders', ops: ['insert', '*'] }),
      row('big-only', { db: 'primary', table: 'orders' }, { filters: { $and: [{ 'input.big': { $eq: true } }] } }),
      row('users', { db: 'primary', table: 'users' }),
    ])
    await sdk.call({
      function_id: 'database::on-row-changed',
      payload: { db: 'primary', table: 'orders', op: 'insert' },
    })
    await flushed()
    expect(reportedItems(socket).map((i) => (i.matched as string[]).sort())).toEqual([
      ['every-insert', 'orders-any', 'orders-insert', 'orders-insert-or-any'],
    ])
  })

  it('computes the keys once per accepted observer and once per call', async () => {
    const { sdk, socket } = worker()
    const observerKeys = vi.fn(byRow.observer)
    const callKeys = vi.fn(byRow.call)
    sdk
      .registerFunction('database::on-row-changed', (change: Change) => change, { private: true })
      .onObserverMatch({ observer: observerKeys, call: callKeys })
    socket.deliver(
      functionRegistered(
        'database::on-row-changed',
        [row('orders', { db: 'primary', table: 'orders' }), row('any', { db: 'primary' })],
        { revision: 1 },
      ),
    )
    await settle()
    socket.deliver(observerAdded('database::on-row-changed', 2, row('users', { db: 'primary', table: 'users' })))
    await settle()
    socket.clear()
    expect(observerKeys.mock.calls.map(([obs]) => obs.id)).toEqual(['orders', 'any', 'users'])
    for (const table of ['orders', 'users', 'items']) {
      await sdk.call({ function_id: 'database::on-row-changed', payload: { db: 'primary', table, op: 'insert' } })
    }
    expect(callKeys).toHaveBeenCalledTimes(3)
    expect(observerKeys).toHaveBeenCalledTimes(3)
    await flushed()
    expect(reportedItems(socket).map((i) => (i.matched as string[]).sort())).toEqual([
      ['any', 'orders'],
      ['any', 'users'],
      ['any'],
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

describe('calls for one observer: metadata.for_observer (decision 52)', () => {
  type Change = { session: string; n: number }
  const forObserver: MatchPredicate<Change, Change> = (obs, call) =>
    call.metadata?.for_observer ? call.metadata.for_observer === obs.id : obs.args.session === call.input.session
  const forObserverKeys: MatchKeys<Change, Change> = {
    observer: (obs) => [obs.args.session as string, `for_observer:${obs.id}`],
    call: (call) => (call.metadata?.for_observer ? `for_observer:${call.metadata.for_observer}` : call.input.session),
  }
  const watch = (id: string) =>
    observerInfo(id, `ui::${id}`, { args: { session: 's1' }, capture: ['input', 'output', 'metadata'] })

  for (const [form, match] of [
    ['a predicate', forObserver],
    ['keys', forObserverKeys],
  ] as const) {
    it(`${form}: the call's metadata reaches the handler, the match hook and the report; only that observer gets it`, async () => {
      const { sdk, socket } = worker()
      const handlerMetadata: unknown[] = []
      const matchMetadata: unknown[] = []
      const recording: MatchPredicate<Change, Change> | MatchKeys<Change, Change> =
        typeof match === 'function'
          ? (obs, call) => {
              matchMetadata.push(call.metadata)
              return match(obs, call)
            }
          : {
              observer: match.observer,
              call: (call: Call<Change, Change>) => {
                matchMetadata.push(call.metadata)
                return match.call(call)
              },
            }
      sdk
        .registerFunction(
          'tmux::on-changed',
          (change: Change, metadata?: unknown) => {
            handlerMetadata.push(metadata)
            return change
          },
          { private: true },
        )
        .onObserverMatch(recording)
      socket.deliver(functionRegistered('tmux::on-changed', [watch('a'), watch('b')], { revision: 1 }))
      await settle()
      socket.clear()

      await sdk.call({
        function_id: 'tmux::on-changed',
        payload: { session: 's1', n: 1 },
        metadata: { for_observer: 'a' },
      })
      await sdk.call({ function_id: 'tmux::on-changed', payload: { session: 's1', n: 2 } })
      await flushed()
      expect(handlerMetadata).toEqual([{ for_observer: 'a' }, undefined])
      expect(matchMetadata).toContainEqual({ for_observer: 'a' })
      expect(reportedItems(socket).map((i) => [(i.matched as string[]).sort(), i.input, i.metadata])).toEqual([
        [['a'], { session: 's1', n: 1 }, { for_observer: 'a' }],
        [['a', 'b'], { session: 's1', n: 2 }, undefined],
      ])

      // routed: the observer it names rides the answer, for the engine to evaluate
      socket.deliver(
        invokeFunction(
          'tmux::on-changed',
          { session: 's1', n: 3 },
          { invocation_id: 'for-b', metadata: { for_observer: 'b' } },
        ),
      )
      await settle()
      expect(fake.resultOf(socket, 'for-b')).toMatchObject({ result: { session: 's1', n: 3 }, matched: ['b'] })
      expect(matchMetadata).toContainEqual({ for_observer: 'b' })
    })
  }

  it('without a match hook, such a call reaches every observer whose filters match, metadata included', async () => {
    const { sdk, socket } = worker()
    sdk.registerFunction('tmux::on-changed', (change: Change) => change, { private: true })
    const ordinary = observerInfo('ordinary', 'ui::ordinary', {
      args: { session: 's1' },
      capture: ['input', 'metadata'],
      filters: { 'metadata.for_observer': { $exists: false } },
    })
    socket.deliver(functionRegistered('tmux::on-changed', [watch('a'), ordinary, watch('c')], { revision: 1 }))
    await settle()
    socket.clear()

    await sdk.call({
      function_id: 'tmux::on-changed',
      payload: { session: 's1', n: 1 },
      metadata: { for_observer: 'a' },
    })
    await sdk.call({ function_id: 'tmux::on-changed', payload: { session: 's1', n: 2 } })
    await flushed()
    expect(reportedItems(socket).map((i) => [(i.matched as string[]).sort(), i.metadata])).toEqual([
      [['a', 'c'], { for_observer: 'a' }],
      [['a', 'c', 'ordinary'], undefined],
    ])
  })

  it('the documented added hook: an accepted observer gets its state, a rejected one nothing, and no rejection goes unhandled', async () => {
    const unhandled: unknown[] = []
    const record = (reason: unknown) => unhandled.push(reason)
    process.on('unhandledRejection', record)
    try {
      const { sdk, socket } = worker()
      const added: Observer[] = []
      sdk
        .registerFunction('tmux::on-changed', (change: Change) => change, { private: true })
        .onObserverAdded((obs) => {
          added.push(obs)
          // as observable-functions.mdx, the README and Observer.accepted() give it
          void obs
            .accepted()
            .then(() =>
              sdk.call({
                function_id: 'tmux::on-changed',
                payload: { session: obs.args.session, n: 0 },
                metadata: { for_observer: obs.id },
                action: CallAction.Void(),
              }),
            )
            .catch(() => {
              // a rejected observer gets nothing
            })
        })
        .onObserverMatch({
          // throws for an observer without a session: rejected after its added hook returned
          observer: (obs) => {
            if (typeof obs.args.session !== 'string') throw new Error('no session')
            return [obs.args.session, `for_observer:${obs.id}`]
          },
          call: forObserverKeys.call,
        })
      const sessionless = observerInfo('sessionless', 'ui::sessionless', { capture: ['input', 'metadata'] })
      socket.deliver(functionRegistered('tmux::on-changed', [watch('a'), sessionless], { revision: 1 }))
      await settle()
      await flushed()
      await settle()

      await expect(added.find((o) => o.id === 'sessionless')?.accepted()).rejects.toMatchObject({
        code: 'OBSERVE_REJECTED',
      })
      expect(reportedItems(socket).map((i) => [i.matched, i.input, i.metadata])).toEqual([
        [['a'], { session: 's1', n: 0 }, { for_observer: 'a' }],
      ])
      expect(unhandled).toEqual([])
    } finally {
      process.off('unhandledRejection', record)
    }
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
    await expect(sdk.call({ function_id: 'http::on-request', payload: { path: '/boom' } })).rejects.toMatchObject({
      code: 'invocation_failed',
      message: 'invocation_failed: no route',
      function_id: 'http::on-request',
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
    ).rejects.toMatchObject({
      code: 'OBSERVER_NOT_ACTIVE',
      message: 'OBSERVER_NOT_ACTIVE: subscriber removed',
      function_id: 'queue::on-message',
    })
    socket.deliver(
      invokeFunction('queue::on-message', { n: 3 }, { invocation_id: 'routed', metadata: { subscriber: 'sub-2' } }),
    )
    await settle()
    expect(fake.resultOf(socket, 'routed')).toEqual({
      type: 'invocationresult',
      invocation_id: 'routed',
      function_id: 'queue::on-message',
      error: { code: 'OBSERVER_NOT_ACTIVE', message: 'subscriber removed' },
      delivered: true,
    })
  })

  it('null removes the hook: iii delivers the calls again, local and routed', async () => {
    const { sdk, socket } = worker()
    const hook = vi.fn((call: Call) => call.output)
    sdk.registerFunction('orders::on-placed', (order: { n: number }) => order).onObserverDeliver(hook)
    socket.deliver(functionRegistered('orders::on-placed', [observerInfo('o', 'audit::order', { capture: ['input'] })]))
    await settle()
    socket.clear()
    await sdk.call({ function_id: 'orders::on-placed', payload: { n: 1 } })
    socket.deliver(invokeFunction('orders::on-placed', { n: 2 }, { invocation_id: 'hooked' }))
    await settle()
    expect(hook).toHaveBeenCalledTimes(2)
    expect(fake.resultOf(socket, 'hooked')).toMatchObject({ result: { n: 2 }, delivered: true })

    sdk.onObserverDeliver('orders::on-placed', null)
    await sdk.call({ function_id: 'orders::on-placed', payload: { n: 3 } })
    socket.deliver(invokeFunction('orders::on-placed', { n: 4 }, { invocation_id: 'delivered' }))
    await settle()
    expect(hook).toHaveBeenCalledTimes(2)
    // without a hook the engine evaluates the routed call's observers: the answer names none
    expect(fake.resultOf(socket, 'delivered')).toEqual({
      type: 'invocationresult',
      invocation_id: 'delivered',
      function_id: 'orders::on-placed',
      result: { n: 4 },
    })
    await flushed()
    expect(reportedItems(socket).map((i) => [i.matched, i.input])).toEqual([[['o'], { n: 3 }]])
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
})

describe('local calls: observers matched through the index of their equality conditions (decision 55)', () => {
  type Event = { tenant: string; n: number; fail?: boolean }
  const tenantObservers = (count: number) =>
    Array.from({ length: count }, (_, i) =>
      observerInfo(`tenant-${i}`, 'audit::tenant', {
        filters: { 'input.tenant': { $eq: `t-${i % 500}` } },
        on: i % 7 === 0 ? 'complete' : 'success',
      }),
    )
  const others = [
    observerInfo('big', 'audit::big', { filters: { 'input.n': { $gte: 10 } } }),
    observerInfo('failures', 'audit::failures', { filters: { 'input.tenant': { $in: ['t-7', 't-8'] } }, on: 'error' }),
    observerInfo('all', 'audit::all'),
  ]

  async function events(observers: Json[]) {
    const { sdk, socket } = worker()
    const ref = sdk.registerFunction('events::on-event', (event: Event) => {
      if (event.fail) throw new Error('failed')
      return event
    })
    socket.deliver(functionRegistered('events::on-event', observers, { revision: 1 }))
    await settle()
    socket.clear()
    return { sdk, socket, ref }
  }

  it("reports the observers each observer's own evaluation accepts, in the order they joined, and evaluates a few filters", async () => {
    const { sdk, socket, ref } = await events([...others.slice(0, 1), ...tenantObservers(1000), ...others.slice(1)])
    const held = ref.observers() as unknown as (Observer & { filter: { matches: (doc: unknown) => boolean } })[]
    expect(held).toHaveLength(1003)
    const spies = held.map((obs) => vi.spyOn(obs.filter, 'matches'))
    const calls: Event[] = [
      { tenant: 't-7', n: 1 },
      { tenant: 't-7', n: 12 },
      { tenant: 't-499', n: 0 },
      { tenant: 'nobody', n: 3 },
      { tenant: 't-8', n: 1, fail: true },
    ]
    const expected: string[][] = []
    for (const input of calls) {
      const call = input.fail
        ? { input, status: 'error' as const, error: { code: 'invocation_failed', message: 'failed' } }
        : { input, output: input, status: 'success' as const }
      expected.push(held.filter((obs) => obs.matches(call)).map((obs) => obs.id))
    }
    for (const spy of spies) spy.mockClear()
    for (const input of calls)
      await sdk.call({ function_id: 'events::on-event', payload: input }).catch(() => undefined)
    await flushed()
    expect(reportedItems(socket).map((i) => i.matched)).toEqual(expected.filter((ids) => ids.length > 0))
    expect(expected[0]).toEqual(['tenant-7', 'tenant-507', 'all'])
    // per call: the two filed under its tenant, and the two observers without an equality condition
    const evaluated = spies.reduce((n, spy) => n + spy.mock.calls.length, 0)
    expect(evaluated).toBeLessThanOrEqual(calls.length * 5)
  })

  it('a call matches the observers active when it started: one that joins or leaves while it runs is delivered as the snapshot says', async () => {
    let release: () => void = () => undefined
    const { sdk, socket } = worker()
    sdk.registerFunction('events::on-event', async (event: Event) => {
      await new Promise<void>((resolve) => {
        release = resolve
      })
      return event
    })
    socket.deliver(functionRegistered('events::on-event', tenantObservers(20), { revision: 1 }))
    await settle()
    socket.clear()
    const during = async (change: Json) => {
      const call = sdk.call({ function_id: 'events::on-event', payload: { tenant: 't-3', n: 1 } })
      await settle()
      socket.deliver(change)
      await settle()
      release()
      await call
      await flushed()
    }
    // joins: not delivered
    await during(
      observerAdded(
        'events::on-event',
        2,
        observerInfo('late', 'audit::late', { filters: { 'input.tenant': { $eq: 't-3' } } }),
      ),
    )
    // leaves: delivered, as the snapshot holds it
    await during(observerRemoved('events::on-event', 3, 'tenant-3'))
    // after both: the index holds the active set
    const after = sdk.call({ function_id: 'events::on-event', payload: { tenant: 't-3', n: 2 } })
    await settle()
    release()
    await after
    await flushed()
    expect(reportedItems(socket).map((i) => i.matched)).toEqual([['tenant-3'], ['tenant-3', 'late'], ['late']])
  })

  it('an observer rejected late leaves the index; added again, it joins at the end', async () => {
    const { sdk, socket, ref } = await events(tenantObservers(10))
    const first = ref.observers()[0]
    first.reject('GONE', 'released')
    await settle()
    await sdk.call({ function_id: 'events::on-event', payload: { tenant: 't-0', n: 1 } })
    socket.deliver(
      observerAdded(
        'events::on-event',
        2,
        observerInfo('tenant-0', 'audit::tenant', { filters: { 'input.tenant': { $eq: 't-1' } } }),
      ),
    )
    await settle()
    await sdk.call({ function_id: 'events::on-event', payload: { tenant: 't-1', n: 1 } })
    await flushed()
    expect(reportedItems(socket).map((i) => i.matched)).toEqual([['tenant-1', 'tenant-0']])
  })
})

describe('routed calls: the answer says how the call is delivered (v2 §8.2, decision 55)', () => {
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

  const observers = [
    observerInfo('big', 'alerts::big', { filters: { $and: [{ 'input.qty': { $gte: 10 } }] } }),
    observerInfo('all', 'audit::all'),
    observerInfo('errors', 'alerts::errors', { on: 'error' }),
  ]

  it('no match hook and no deliver hook: neither matched nor delivered, on success and on the handler error; no report', async () => {
    const { socket } = await routed(observers)
    socket.deliver(invokeFunction('orders::create', { qty: 12 }, { invocation_id: 'i-1' }))
    socket.deliver(invokeFunction('orders::create', { qty: 1, fail: true }, { invocation_id: 'i-2' }))
    await settle()
    await flushed()
    // the engine evaluates on and filters of every active observer: the host evaluates none
    expect(socket.frames()).toEqual([
      {
        type: 'invocationresult',
        invocation_id: 'i-1',
        function_id: 'orders::create',
        result: { order_id: 'o-1', qty: 12 },
      },
      {
        type: 'invocationresult',
        invocation_id: 'i-2',
        function_id: 'orders::create',
        error: { code: 'invocation_failed', message: 'out of stock', stacktrace: expect.any(String) },
      },
    ])
  })

  it('evaluates no filter for a routed call; a local call evaluates them', async () => {
    let ref: FunctionRef<Order, Created> | undefined
    const { sdk, socket } = await routed(observers, (r) => {
      ref = r
    })
    // each observer's compiled filter (internal)
    const held = ref?.observers() as unknown as { filter: { matches: (doc: unknown) => boolean } }[]
    expect(held).toHaveLength(3)
    const spies = held.map((obs) => vi.spyOn(obs.filter, 'matches'))
    const evaluated = () => spies.reduce((n, spy) => n + spy.mock.calls.length, 0)
    socket.deliver(invokeFunction('orders::create', { qty: 12 }, { invocation_id: 'i-1' }))
    await settle()
    expect(answer(socket, 'i-1')).toMatchObject({ result: { order_id: 'o-1', qty: 12 } })
    expect(evaluated()).toBe(0)
    await sdk.call({ function_id: 'orders::create', payload: { qty: 12 } })
    expect(evaluated()).toBe(2) // `big` and `all`; the `on: error` observer is ruled out first
  })

  it('a predicate match hook: matched lists the observers it matched, before on and filters, on success and on error', async () => {
    const asked: string[] = []
    const { socket } = await routed(
      [
        observerInfo('small', 'audit::small', { args: { max: 5 } }),
        observerInfo('large', 'audit::large', { args: { max: 50 }, filters: { 'input.qty': { $gte: 1000 } } }),
        observerInfo('errors', 'alerts::errors', { args: { max: 50 }, on: 'error' }),
      ],
      (ref) =>
        ref.onObserverMatch((obs, call) => {
          asked.push(`${obs.id}:${call.status}`)
          return call.input.qty <= (obs.args.max as number)
        }),
    )
    socket.deliver(invokeFunction('orders::create', { qty: 10 }, { invocation_id: 'i-1' }))
    await settle()
    socket.deliver(invokeFunction('orders::create', { qty: 10, fail: true }, { invocation_id: 'i-2' }))
    await settle()
    expect(answer(socket, 'i-1')).toEqual({
      type: 'invocationresult',
      invocation_id: 'i-1',
      function_id: 'orders::create',
      result: { order_id: 'o-1', qty: 10 },
      matched: ['large', 'errors'],
    })
    expect(answer(socket, 'i-2')).toMatchObject({
      error: { code: 'invocation_failed', message: 'out of stock' },
      matched: ['large', 'errors'],
    })
    expect(asked).toEqual([
      'small:success',
      'large:success',
      'errors:success',
      'small:error',
      'large:error',
      'errors:error',
    ])
  })

  it('a match hook by keys: matched lists the observers filed under the call keys', async () => {
    const { socket } = await routed(
      [
        observerInfo('eu', 'audit::eu', { args: { region: 'eu' } }),
        observerInfo('us', 'audit::us', { args: { region: 'us' }, on: 'error' }),
        observerInfo('eu-2', 'audit::eu-2', { args: { region: 'eu' } }),
      ],
      (ref) =>
        ref.onObserverMatch({
          observer: (obs) => obs.args.region as string,
          call: (call) => (call.input.qty > 5 ? 'eu' : 'us'),
        }),
    )
    socket.deliver(invokeFunction('orders::create', { qty: 10 }, { invocation_id: 'eu-call' }))
    socket.deliver(invokeFunction('orders::create', { qty: 1 }, { invocation_id: 'us-call' }))
    await settle()
    expect((answer(socket, 'eu-call')?.matched as string[]).sort()).toEqual(['eu', 'eu-2'])
    expect(answer(socket, 'us-call')).toMatchObject({ matched: ['us'] })
  })

  it('a match hook that matches nothing, or that runs with no observer, sends matched: []', async () => {
    const { sdk, socket } = await routed([observerInfo('all', 'audit::all')], (ref) => ref.onObserverMatch(() => false))
    socket.deliver(invokeFunction('orders::create', { qty: 1 }, { invocation_id: 'none' }))
    await settle()
    expect(answer(socket, 'none')).toEqual({
      type: 'invocationresult',
      invocation_id: 'none',
      function_id: 'orders::create',
      result: { order_id: 'o-1', qty: 1 },
      matched: [],
    })
    sdk.registerFunction('orders::archive', async () => 'archived').onObserverMatch(() => true)
    socket.deliver(functionRegistered('orders::archive'))
    await settle()
    socket.deliver(invokeFunction('orders::archive', {}, { invocation_id: 'empty' }))
    await settle()
    expect(answer(socket, 'empty')).toMatchObject({ result: 'archived', matched: [] })
  })

  it('with no observer and no deliver hook, a keys match hook does not run: matched: [], on success and on error', async () => {
    const errors = vi.spyOn(console, 'error').mockImplementation(() => undefined)
    let keyed = 0
    const { sdk, socket } = await routed([], (ref) =>
      ref.onObserverMatch({
        observer: () => 'k',
        call: () => {
          keyed++
          throw new Error('the call keys are not read')
        },
      }),
    )
    socket.deliver(invokeFunction('orders::create', { qty: 1 }, { invocation_id: 'ok' }))
    socket.deliver(invokeFunction('orders::create', { qty: 1, fail: true }, { invocation_id: 'failed' }))
    await settle()
    await expect(sdk.call({ function_id: 'orders::create', payload: { qty: 2 } })).resolves.toEqual({
      order_id: 'o-1',
      qty: 2,
    })
    await flushed()
    expect(answer(socket, 'ok')).toEqual({
      type: 'invocationresult',
      invocation_id: 'ok',
      function_id: 'orders::create',
      result: { order_id: 'o-1', qty: 1 },
      matched: [],
    })
    expect(answer(socket, 'failed')).toEqual({
      type: 'invocationresult',
      invocation_id: 'failed',
      function_id: 'orders::create',
      error: { code: 'invocation_failed', message: 'out of stock', stacktrace: expect.any(String) },
      matched: [],
    })
    expect(keyed).toBe(0)
    expect(errors).not.toHaveBeenCalled()
    expect(reportedItems(socket)).toEqual([])
  })

  it('a match hook that throws matches nothing: matched: []', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined)
    const { socket } = await routed([observerInfo('all', 'audit::all')], (ref) =>
      ref.onObserverMatch(() => {
        throw new Error('no match')
      }),
    )
    socket.deliver(invokeFunction('orders::create', { qty: 1 }, { invocation_id: 'i-1' }))
    await settle()
    expect(answer(socket, 'i-1')).toMatchObject({ result: { order_id: 'o-1', qty: 1 }, matched: [] })
  })

  it('removing the match hook leaves the routed call to the engine: no matched', async () => {
    let ref: FunctionRef<Order, Created> | undefined
    const { socket } = await routed([observerInfo('all', 'audit::all')], (r) => {
      ref = r.onObserverMatch(() => true)
    })
    socket.deliver(invokeFunction('orders::create', { qty: 1 }, { invocation_id: 'hooked' }))
    await settle()
    ref?.onObserverMatch(null)
    socket.deliver(invokeFunction('orders::create', { qty: 2 }, { invocation_id: 'plain' }))
    await settle()
    expect(answer(socket, 'hooked')).toMatchObject({ matched: ['all'] })
    expect(answer(socket, 'plain')).not.toHaveProperty('matched')
    expect(answer(socket, 'plain')).not.toHaveProperty('delivered')
  })

  it('a deliver hook: delivered: true and what the hook returns or throws, never matched', async () => {
    const { socket } = await routed([observerInfo('all', 'audit::all', { on: 'complete' })], (ref) =>
      ref
        .onObserverMatch(() => true)
        .onObserverDeliver(async (call) => {
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
      delivered: true,
    })
    expect(answer(socket, 'rejected')).toEqual({
      type: 'invocationresult',
      invocation_id: 'rejected',
      function_id: 'orders::create',
      error: { code: 'invocation_failed', message: 'rejected by the hook', stacktrace: expect.any(String) },
      delivered: true,
    })
  })

  it('a deliver hook that runs with no observer still answers delivered: true', async () => {
    const { socket } = await routed([], (ref) => ref.onObserverDeliver((call) => call.output))
    socket.deliver(invokeFunction('orders::create', { qty: 3 }, { invocation_id: 'i-1' }))
    await settle()
    expect(answer(socket, 'i-1')).toEqual({
      type: 'invocationresult',
      invocation_id: 'i-1',
      function_id: 'orders::create',
      result: { order_id: 'o-1', qty: 3 },
      delivered: true,
    })
  })

  it('an activation of a hosted target is answered the same way', async () => {
    const { socket } = await routed([observerInfo('all', 'audit::all')], (ref) => ref.onObserverMatch(() => true))
    socket.deliver(
      invokeFunction(
        'orders::create',
        { qty: 4 },
        { invocation_id: 'activation', observe_depth: 1, activation: { observer_id: 'up', activation_id: 'a-1' } },
      ),
    )
    await settle()
    expect(answer(socket, 'activation')).toMatchObject({ result: { order_id: 'o-1', qty: 4 }, matched: ['all'] })
  })

  it('against an engine without observe.v2 the engine evaluates routed calls: neither field', async () => {
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

  it('takes no metadata: refused before anything is sent, whatever the value; null is no metadata (decision 53)', async () => {
    const { sdk, socket } = await http()
    const [obs] = sdk.observers('http::on-request')
    for (const metadata of [{ route: 'users' }, ['a'], 'tag', 0, false]) {
      await expect(sdk.call({ observer: obs, payload: { path: '/users' }, metadata } as never)).rejects.toThrow(
        new Error("a call through an observer takes no metadata; its target receives the observer's metadata"),
      )
    }
    expect(socket.framesOf('invokefunction')).toEqual([])
    await sdk.call({ observer: obs, payload: { path: '/users' }, metadata: null, action: CallAction.Void() } as never)
    expect(socket.framesOf('invokefunction')).toEqual([
      {
        type: 'invokefunction',
        function_id: 'users::get',
        data: { path: '/users' },
        action: { type: 'void' },
        activation: { observer_id: 'users' },
      },
    ])
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
    await expect(response).rejects.toMatchObject({
      code: 'OBSERVER_NOT_ACTIVE',
      message: 'OBSERVER_NOT_ACTIVE: observer users is not active',
      function_id: 'http::on-request',
    })
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
      delivered: true,
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
