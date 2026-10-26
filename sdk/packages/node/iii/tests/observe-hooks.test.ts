import { afterEach, describe, expect, it, vi } from 'vitest'
import type { Observer } from '../src/observe-types'
import type { IIIClient } from '../src/types'

/** Owner hooks against a fake engine (sdk.md §1, §4.1, §4.2, CONTRACT D11). */
vi.mock('ws', async () => (await import('./observe-fake-engine')).wsModule())
vi.resetModules()
const fake = await import('./observe-fake-engine')
const { registerWorker } = await import('../src/index')
const { ObserveRejection, InvocationError } = await import('../src/errors')
const { functionRegistered, observerInfo, observerAdded, observerRemoved, settle } = fake

let sdk: IIIClient | undefined
afterEach(async () => {
  vi.restoreAllMocks()
  vi.useRealTimers()
  await sdk?.shutdown()
  sdk = undefined
})

function worker(hookTimeoutMs?: number) {
  sdk = registerWorker('ws://hooks.test', {
    otel: { enabled: false },
    enableMetricsReporting: false,
    reconnectionConfig: { initialDelayMs: 10, jitterFactor: 0 },
    observe: { hookTimeoutMs },
  } as never)
  const socket = fake.lastSocket()
  return { sdk, socket }
}

function deferred<T = void>() {
  let resolve!: (v: T) => void
  let reject!: (e: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

const acks = (socket: ReturnType<typeof fake.lastSocket>) => socket.framesOf('observerack')

describe('onObserverAdded / onObserverRemoved', () => {
  it('runs onObserverAdded once per add, and the observer is in the compiled set before the ack', async () => {
    const { sdk, socket } = worker()
    let observedAtAck: boolean | undefined
    const added: Observer[] = []
    const ref = sdk.registerFunction('cron::on-tick', async () => null, {
      observe: {
        delivery: 'owner',
        onObserverAdded: (obs) => {
          added.push(obs)
          expect(acks(socket)).toHaveLength(0) // before the ack
        },
      },
    })
    socket.open()
    const send = socket.send.bind(socket)
    vi.spyOn(socket, 'send').mockImplementation((data, cb) => {
      if (String(data).includes('"type":"observerack"')) observedAtAck = ref.isObserved()
      send(data, cb)
    })
    expect(ref.isObserved()).toBe(false)
    socket.deliver(functionRegistered('cron::on-tick', [], { revision: 1 }))
    socket.deliver(
      observerAdded(
        'cron::on-tick',
        2,
        observerInfo('o-1', 'local::cron-handler', {
          args: { expression: '* * * * * *' },
          filters: { $and: [{ 'input.n': { $gt: 1 } }] },
          capture: ['input'],
        }),
      ),
    )
    await settle()
    expect(added).toHaveLength(1)
    expect(added[0]).toMatchObject({
      id: 'o-1',
      function_id: 'cron::on-tick',
      args: { expression: '* * * * * *' },
      filters: { $and: [{ 'input.n': { $gt: 1 } }] },
      target: 'local::cron-handler',
      capture: ['input'],
      on: 'success',
    })
    expect(added[0]).not.toHaveProperty('deliver')
    expect(acks(socket)).toEqual([
      { type: 'observerack', function_id: 'cron::on-tick', namespace: 'default', id: 'o-1', revision: 2 },
    ])
    expect(observedAtAck).toBe(true)
    expect(ref.isObserved()).toBe(true)
    expect(ref.observers()).toEqual([added[0]])
    expect(added[0].matches({ input: { n: 2 } })).toBe(true)
    expect(added[0].matches({ input: { n: 1 } })).toBe(false)
  })

  it('rejects with the ObserveRejection code, or OBSERVE_REJECTED for a plain error', async () => {
    const { sdk, socket } = worker()
    const ref = sdk.registerFunction('cron::on-tick', async () => null, {
      observe: {
        delivery: 'owner',
        onObserverAdded: (obs) => {
          if (obs.id === 'bad-expr') throw new ObserveRejection('INVALID_CRON_EXPRESSION', 'bad field')
          throw new Error('nope')
        },
      },
    })
    socket.open()
    socket.deliver(
      functionRegistered('cron::on-tick', [observerInfo('bad-expr', 't::a'), observerInfo('plain', 't::b')], {
        revision: 3,
      }),
    )
    await settle()
    expect(acks(socket).map((a) => [a.id, a.error])).toEqual([
      ['bad-expr', { code: 'INVALID_CRON_EXPRESSION', message: 'bad field' }],
      ['plain', { code: 'OBSERVE_REJECTED', message: 'nope' }],
    ])
    expect(ref.isObserved()).toBe(false)
  })

  it('rejects with OWNER_TIMEOUT when onObserverAdded runs too long, and undoes a late success', async () => {
    fake.fakeClock()
    const { sdk, socket } = worker(40)
    const slow = deferred()
    const removed = vi.fn()
    const ref = sdk.registerFunction('cron::on-tick', async () => null, {
      observe: { delivery: 'owner', onObserverAdded: () => slow.promise, onObserverRemoved: removed },
    })
    socket.open()
    socket.deliver(functionRegistered('cron::on-tick', [observerInfo('o-1', 't::a')], { revision: 1 }))
    await settle()
    expect(acks(socket)).toEqual([])
    await fake.elapse(40)
    expect(acks(socket)).toEqual([
      expect.objectContaining({ id: 'o-1', error: expect.objectContaining({ code: 'OWNER_TIMEOUT' }) }),
    ])
    expect(ref.isObserved()).toBe(false)
    slow.resolve()
    await settle()
    expect(removed).toHaveBeenCalledTimes(1)
  })

  it('runs the hooks of one function one at a time, in revision order', async () => {
    const { sdk, socket } = worker()
    const first = deferred()
    const order: string[] = []
    sdk.registerFunction('cron::on-tick', async () => null, {
      observe: {
        delivery: 'owner',
        onObserverAdded: async (obs) => {
          order.push(`start ${obs.id}`)
          if (obs.id === 'o-1') await first.promise
          order.push(`end ${obs.id}`)
        },
        onObserverRemoved: (obs) => {
          order.push(`removed ${obs.id}`)
        },
      },
    })
    socket.open()
    socket.deliver(functionRegistered('cron::on-tick', [], { revision: 0 }))
    socket.deliver(observerAdded('cron::on-tick', 1, observerInfo('o-1', 't::a')))
    socket.deliver(observerAdded('cron::on-tick', 2, observerInfo('o-2', 't::b')))
    socket.deliver(observerRemoved('cron::on-tick', 3, 'o-1'))
    await settle()
    expect(order).toEqual(['start o-1'])
    first.resolve()
    await settle()
    expect(order).toEqual(['start o-1', 'end o-1', 'start o-2', 'end o-2', 'removed o-1'])
    expect(acks(socket).map((a) => [a.id, a.revision])).toEqual([
      ['o-1', 1],
      ['o-2', 2],
    ])
  })

  it('runs the hooks of different functions concurrently', async () => {
    const { sdk, socket } = worker()
    const blocked = deferred()
    const started: string[] = []
    for (const id of ['a::on-x', 'b::on-y']) {
      sdk.registerFunction(id, async () => null, {
        observe: {
          delivery: 'owner',
          onObserverAdded: async () => {
            started.push(id)
            await blocked.promise
          },
        },
      })
    }
    socket.open()
    socket.deliver(functionRegistered('a::on-x', [observerInfo('o-a', 't::a')]))
    socket.deliver(functionRegistered('b::on-y', [observerInfo('o-b', 't::b')]))
    await settle()
    expect(started.sort()).toEqual(['a::on-x', 'b::on-y'])
    blocked.resolve()
  })

  it('ignores added / removed messages whose revision is not greater than the one it holds', async () => {
    const { sdk, socket } = worker()
    const added = vi.fn()
    const removed = vi.fn()
    const ref = sdk.registerFunction('cron::on-tick', async () => null, {
      observe: { delivery: 'owner', onObserverAdded: added, onObserverRemoved: removed },
    })
    socket.open()
    socket.deliver(functionRegistered('cron::on-tick', [observerInfo('o-1', 't::a')], { revision: 5 }))
    socket.deliver(observerAdded('cron::on-tick', 5, observerInfo('o-2', 't::b')))
    socket.deliver(observerAdded('cron::on-tick', 4, observerInfo('o-3', 't::c')))
    socket.deliver(observerRemoved('cron::on-tick', 5, 'o-1'))
    await settle()
    expect(added).toHaveBeenCalledTimes(1)
    expect(removed).not.toHaveBeenCalled()
    expect(ref.observers().map((o) => o.id)).toEqual(['o-1'])

    socket.deliver(observerRemoved('cron::on-tick', 6, 'o-1'))
    await settle()
    expect(removed).toHaveBeenCalledTimes(1)
    expect(ref.isObserved()).toBe(false)
  })

  it('diffs a fresh functionregistered after a reconnect: removed, added, kept', async () => {
    vi.useFakeTimers()
    const { sdk, socket } = worker()
    const added: string[] = []
    const removed: string[] = []
    const ref = sdk.registerFunction('cron::on-tick', async () => null, {
      observe: {
        delivery: 'owner',
        onObserverAdded: (obs) => {
          added.push(obs.id)
        },
        onObserverRemoved: (obs) => {
          removed.push(obs.id)
        },
      },
    })
    socket.open()
    socket.deliver(
      functionRegistered('cron::on-tick', [observerInfo('a', 't::a'), observerInfo('b', 't::b')], { revision: 9 }),
    )
    await vi.advanceTimersByTimeAsync(1)
    const keptBefore = ref.observers().find((o) => o.id === 'b')

    socket.drop()
    await vi.advanceTimersByTimeAsync(50)
    const next = fake.lastSocket()
    next.open()
    // revisions restart after an engine restart: functionregistered is always accepted
    next.deliver(
      functionRegistered(
        'cron::on-tick',
        [observerInfo('b', 't::b', { filters: { $and: [{ 'input.x': { $eq: 1 } }] } }), observerInfo('c', 't::c')],
        { revision: 2 },
      ),
    )
    await vi.advanceTimersByTimeAsync(1)
    expect(added).toEqual(['a', 'b', 'c'])
    expect(removed).toEqual(['a'])
    expect(ref.observers().map((o) => o.id)).toEqual(['b', 'c'])
    const kept = ref.observers().find((o) => o.id === 'b')
    expect(kept).toBe(keptBefore) // the same object, updated in place
    expect(kept?.filters).toEqual({ $and: [{ 'input.x': { $eq: 1 } }] })
    expect(next.framesOf('observerack').map((a) => [a.id, a.revision, a.error])).toEqual([
      ['b', 2, undefined],
      ['c', 2, undefined],
    ])
  })

  it('treats changed args as a different observer (removed, then added)', async () => {
    const { sdk, socket } = worker()
    const calls: string[] = []
    sdk.registerFunction('cron::on-tick', async () => null, {
      observe: {
        delivery: 'owner',
        onObserverAdded: (obs) => {
          calls.push(`added ${JSON.stringify(obs.args)}`)
        },
        onObserverRemoved: (obs) => {
          calls.push(`removed ${JSON.stringify(obs.args)}`)
        },
      },
    })
    socket.open()
    socket.deliver(functionRegistered('cron::on-tick', [observerInfo('a', 't::a', { args: { e: 1 } })]))
    socket.deliver(functionRegistered('cron::on-tick', [observerInfo('a', 't::a', { args: { e: 2 } })]))
    await settle()
    expect(calls).toEqual(['added {"e":1}', 'removed {"e":1}', 'added {"e":2}'])
  })

  it('FUNCTION_NAMESPACE_CONFLICT clears local dispatch and the observer set', async () => {
    const { sdk, socket } = worker()
    const removed = vi.fn()
    const handler = vi.fn(async () => 'ok')
    const ref = sdk.registerFunction('orders::archive', handler, { observe: { onObserverRemoved: removed } })
    socket.open()
    socket.deliver(functionRegistered('orders::archive', [observerInfo('a', 't::a')]))
    await settle()
    expect(ref.isObserved()).toBe(true)
    await sdk.trigger({ function_id: 'orders::archive', payload: {} })
    expect(handler).toHaveBeenCalledTimes(1) // local

    socket.deliver({
      type: 'registrationrejected',
      code: 'FUNCTION_NAMESPACE_CONFLICT',
      namespace: 'default',
      function_id: 'orders::archive',
      owner_worker_id: 'other',
    })
    await settle()
    expect(ref.isObserved()).toBe(false)
    expect(removed).toHaveBeenCalledTimes(1)
    socket.clear()
    void sdk.trigger({ function_id: 'orders::archive', payload: {}, timeoutMs: 50 }).catch(() => undefined)
    expect(socket.framesOf('invokefunction')).toHaveLength(1) // remote now
    expect(handler).toHaveBeenCalledTimes(1)
  })

  it('unregister removes the observers through onObserverRemoved', async () => {
    const { sdk, socket } = worker()
    const removed = vi.fn()
    const ref = sdk.registerFunction('orders::archive', async () => null, {
      observe: { onObserverRemoved: removed },
    })
    socket.open()
    socket.deliver(functionRegistered('orders::archive', [observerInfo('a', 't::a'), observerInfo('b', 't::b')]))
    await settle()
    ref.unregister()
    await settle()
    expect(removed).toHaveBeenCalledTimes(2)
    expect(ref.isObserved()).toBe(false)
  })

  it('logs an onObserverRemoved error and goes on', async () => {
    const { sdk, socket } = worker()
    const errors = vi.spyOn(console, 'error').mockImplementation(() => undefined)
    const ref = sdk.registerFunction('orders::archive', async () => null, {
      observe: {
        onObserverRemoved: () => {
          throw new Error('cleanup failed')
        },
      },
    })
    socket.open()
    socket.deliver(functionRegistered('orders::archive', [observerInfo('a', 't::a')], { revision: 1 }))
    socket.deliver(observerRemoved('orders::archive', 2, 'a'))
    socket.deliver(observerAdded('orders::archive', 3, observerInfo('b', 't::b')))
    await settle()
    expect(errors.mock.calls.some((c) => String(c[0]).includes('onObserverRemoved failed'))).toBe(true)
    expect(ref.observers().map((o) => o.id)).toEqual(['b'])
  })

  it('acks INVALID_FILTER for filters that are not an object (never match-all)', async () => {
    const { sdk, socket } = worker()
    const added = vi.fn()
    const ref = sdk.registerFunction('orders::archive', async () => null, { observe: { onObserverAdded: added } })
    socket.open()
    socket.deliver(
      functionRegistered('orders::archive', [
        { ...observerInfo('null-filters', 't::a'), filters: null },
        { ...observerInfo('string-filters', 't::b'), filters: 'input.a' },
        { ...observerInfo('no-filters', 't::c'), filters: undefined },
      ]),
    )
    await settle()
    expect(acks(socket).map((a) => [a.id, (a.error as { code?: string } | undefined)?.code])).toEqual([
      ['null-filters', 'INVALID_FILTER'],
      ['string-filters', 'INVALID_FILTER'],
      ['no-filters', undefined],
    ])
    expect(added).toHaveBeenCalledTimes(1)
    expect(ref.observers().map((o) => o.id)).toEqual(['no-filters'])
  })

  it('a kept observer whose new filters do not compile is acked INVALID_FILTER at the new revision, which reject() then uses', async () => {
    const { sdk, socket } = worker()
    let obs: Observer | undefined
    sdk.registerFunction('cron::on-tick', async () => null, {
      observe: {
        delivery: 'owner',
        onObserverAdded: (o) => {
          obs = o
        },
      },
    })
    socket.open()
    socket.deliver(functionRegistered('cron::on-tick', [observerInfo('a', 't::a')], { revision: 3 }))
    socket.deliver(
      functionRegistered('cron::on-tick', [{ ...observerInfo('a', 't::a'), filters: { 'input.x': 1 } }], {
        revision: 5,
      }),
    )
    await settle()
    obs?.reject('GONE', 'no longer wanted')
    expect(acks(socket).map((a) => [a.revision, (a.error as { code?: string } | undefined)?.code])).toEqual([
      [3, undefined],
      [5, 'INVALID_FILTER'],
      [5, 'GONE'],
    ])
  })

  it('a rejection made while onObserverAdded is still running wins over the add', async () => {
    const { sdk, socket } = worker()
    const gate = deferred()
    const pending: Observer[] = []
    const ref = sdk.registerFunction('cron::on-tick', async () => null, {
      observe: {
        delivery: 'owner',
        onObserverAdded: async (obs) => {
          if (obs.id === 'sync') obs.reject('BAD_CONFIG', 'rejected inside the hook')
          if (obs.id === 'async') {
            pending.push(obs)
            await gate.promise
          }
        },
      },
    })
    socket.open()
    socket.deliver(
      functionRegistered('cron::on-tick', [observerInfo('sync', 't::a'), observerInfo('async', 't::b')], {
        revision: 2,
      }),
    )
    await settle()
    pending[0].reject('RELOADED', 'rejected while the hook ran') // from elsewhere, before the hook returns
    gate.resolve()
    await settle()
    expect(acks(socket).map((a) => [a.id, a.revision, a.error])).toEqual([
      ['sync', 2, { code: 'BAD_CONFIG', message: 'rejected inside the hook' }],
      ['async', 2, { code: 'RELOADED', message: 'rejected while the hook ran' }],
    ])
    expect(ref.isObserved()).toBe(false)
    // and it stays rejected when the set is replayed
    socket.clear()
    socket.deliver(functionRegistered('cron::on-tick', [observerInfo('async', 't::b')], { revision: 4 }))
    await settle()
    expect(acks(socket).map((a) => [a.id, a.revision, (a.error as { code?: string }).code])).toEqual([
      ['async', 4, 'RELOADED'],
    ])
    expect(ref.isObserved()).toBe(false)
  })

  it('a conflict arriving while onObserverAdded runs: the add is undone (onObserverRemoved, no success ack, not active)', async () => {
    const { sdk, socket } = worker()
    const gate = deferred()
    const calls: string[] = []
    const ref = sdk.registerFunction('cron::on-tick', async () => null, {
      observe: {
        delivery: 'owner',
        onObserverAdded: async (obs) => {
          calls.push(`added ${obs.id}`)
          if (obs.id === 'slow') await gate.promise
        },
        onObserverRemoved: (obs) => {
          calls.push(`removed ${obs.id}`)
        },
      },
    })
    socket.open()
    socket.deliver(
      functionRegistered('cron::on-tick', [observerInfo('done', 't::a'), observerInfo('slow', 't::b')], {
        revision: 1,
      }),
    )
    socket.deliver(observerAdded('cron::on-tick', 2, observerInfo('queued', 't::c'))) // queued behind the slow hook
    await settle()
    expect(calls).toEqual(['added done', 'added slow'])
    socket.deliver({
      type: 'registrationrejected',
      code: 'FUNCTION_NAMESPACE_CONFLICT',
      namespace: 'default',
      function_id: 'cron::on-tick',
      owner_worker_id: 'other',
    })
    expect(ref.isObserved()).toBe(false) // delivery stops at once
    gate.resolve()
    await settle()
    expect(calls).toEqual(['added done', 'added slow', 'removed slow', 'removed done'])
    expect(acks(socket).map((a) => [a.id, a.error])).toEqual([['done', undefined]])
    expect(ref.isObserved()).toBe(false)
    expect(ref.observers()).toEqual([])
  })

  it('carries the legacy trigger config of a shim observer', async () => {
    const { sdk, socket } = worker()
    const added: Observer[] = []
    sdk.registerFunction('http::on-request', async () => null, {
      observe: {
        delivery: 'owner',
        legacy_trigger_type: 'http',
        onObserverAdded: (obs) => {
          added.push(obs)
        },
      },
    })
    socket.open()
    socket.deliver(functionRegistered('http::on-request', [], { revision: 3 }))
    socket.deliver({
      type: 'observeradded',
      function_id: 'http::on-request',
      namespace: 'default',
      revision: 4,
      observer: {
        ...observerInfo('legacy-1', 'users::get'),
        legacy: { trigger_type: 'http', config: { api_path: '/users/:id', http_method: 'GET' } },
      },
    })
    await settle()
    expect(added[0].legacy).toEqual({ trigger_type: 'http', config: { api_path: '/users/:id', http_method: 'GET' } })
  })
})

describe('the observe descriptor', () => {
  it('throws INVALID_OBSERVE_DESCRIPTOR for args_format on an auto function', () => {
    const { sdk } = worker()
    const register = (observe: object) => () => sdk.registerFunction('x::y', async () => null, { observe } as never)
    expect(register({ args_format: { type: 'object' } })).toThrow(InvocationError)
    expect(register({ delivery: 'auto', args_format: { type: 'object' } })).toThrow(/INVALID_OBSERVE_DESCRIPTOR/)
    expect(register({ delivery: 'sometimes' })).toThrow(/INVALID_OBSERVE_DESCRIPTOR/)
  })

  it('against an engine without observe.v1: registers a plain function and logs once that its hooks are inactive', () => {
    const { sdk, socket } = worker()
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    sdk.registerFunction('cron::on-tick', async () => null, { observe: { delivery: 'owner' } })
    sdk.registerFunction('plain::fn', async () => null)
    socket.open('legacy')
    sdk.registerFunction('db::on-row', async () => null, { observe: { onObserverAdded: () => undefined } })
    const inactive = warn.mock.calls.map((c) => String(c[0])).filter((m) => m.includes('hooks are inactive'))
    expect(inactive).toHaveLength(2)
    expect(inactive[0]).toContain('cron::on-tick')
    expect(inactive[1]).toContain('db::on-row')
    expect(socket.framesOf('registerfunction').map((f) => f.id)).toEqual(['cron::on-tick', 'plain::fn', 'db::on-row'])
  })
})

describe('FunctionObservers edge cases', () => {
  it('a functionregistered diff that throws still confirms the function and flushes its held reports', async () => {
    const { FunctionObservers } = await import('../src/observable')
    const { Reporter } = await import('../src/reporter')
    const reporter = new Reporter({
      online: () => true,
      status: () => 'ready',
      sendFrame: () => true,
      pendingBytes: () => 0,
      logError: () => undefined,
    })
    const flush = vi.spyOn(reporter, 'flush')
    const errors: string[] = []
    const state = new FunctionObservers('f::boom', undefined, true, true, {
      identity: () => ({ worker_name: 'w' }),
      sendAck: () => {
        throw new Error('ack failed')
      },
      reporter,
      callTarget: async () => null,
      logError: (message) => errors.push(message),
      hookTimeoutMs: () => 30_000,
      traceContext: () => ({}),
    })
    state.onFunctionRegistered(
      { namespace: 'default', revision: 1, local_dispatch: true, observers: [observerInfo('a', 't::a') as never] },
      7,
    )
    await settle()
    expect(state.confirmedEpoch).toBe(7)
    expect(flush).toHaveBeenCalled()
    expect(errors.some((m) => m.includes('f::boom'))).toBe(true)
    reporter.stop()
  })

  it('a stale FunctionRef.unregister() leaves the registration that replaced it alone', async () => {
    fake.fakeClock()
    const { sdk, socket } = worker()
    const first = sdk.registerFunction('f::twice', async (n: number) => n)
    socket.open()
    first.unregister()
    const second = sdk.registerFunction('f::twice', async (n: number) => n)
    socket.deliver(functionRegistered('f::twice', [observerInfo('o', 'audit::any', { capture: ['input'] })]))
    await settle()
    socket.drop() // hold the new registration's reports
    await sdk.trigger({ function_id: 'f::twice', payload: 1 })
    const reporter = (sdk as unknown as { reporter: { size: number } }).reporter
    expect(reporter.size).toBe(1)

    first.unregister() // stale: the id belongs to `second` now
    expect(reporter.size).toBe(1)
    expect(second.isObserved()).toBe(true)
    await fake.elapse(1500)
    const next = fake.lastSocket()
    next.open()
    expect(next.framesOf('unregisterfunction')).toEqual([])
    expect(next.framesOf('registerfunction').map((f) => f.id)).toEqual(['f::twice'])
    next.deliver(functionRegistered('f::twice', [observerInfo('o', 'audit::any', { capture: ['input'] })]))
    await settle()
    await fake.elapse(10)
    expect(fake.reportedItems(next).map((i) => i.input)).toEqual([1])
  })
})
