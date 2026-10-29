import { afterEach, describe, expect, it, vi } from 'vitest'
import type { Observer } from '../src/observe-types'
import type { IIIClient } from '../src/types'

/** Observer hooks against a fake engine (v2 §4.2, §4.3, §7, §11; decisions 18, 21, 41, 44). */
vi.mock('ws', async () => (await import('./observe-fake-engine')).wsModule())
vi.resetModules()
const fake = await import('./observe-fake-engine')
const { registerWorker } = await import('../src/index')
const { ObserveRejection } = await import('../src/errors')
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

const warnings = () =>
  vi
    .spyOn(console, 'warn')
    .mockImplementation(() => undefined)

describe('onObserverAdded / onObserverRemoved', () => {
  it('runs onObserverAdded once per add, and the observer is in the compiled set before the ack', async () => {
    const { sdk, socket } = worker()
    let observedAtAck: boolean | undefined
    const added: Observer[] = []
    const ref = sdk.registerFunction('cron::on-tick', async () => null).onObserverAdded((obs) => {
      added.push(obs)
      expect(acks(socket)).toHaveLength(0) // before the ack
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
    // `on` is matched against the call's status (an absent one reads as success)
    expect(added[0].matches({ status: 'error', input: { n: 2 } })).toBe(false)
    await expect(added[0].accepted()).resolves.toBeUndefined()
  })

  it('rejects with the ObserveRejection code, or OBSERVE_REJECTED for a plain error', async () => {
    const { sdk, socket } = worker()
    const seen: Observer[] = []
    const ref = sdk.registerFunction('cron::on-tick', async () => null).onObserverAdded((obs) => {
      seen.push(obs)
      if (obs.id === 'bad-expr') throw new ObserveRejection('INVALID_CRON_EXPRESSION', 'bad field')
      throw new Error('nope')
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
    await expect(seen[0].accepted()).rejects.toMatchObject({ code: 'INVALID_CRON_EXPRESSION' })
    await expect(seen[1].accepted()).rejects.toMatchObject({ code: 'OBSERVE_REJECTED' })
  })

  it('rejects with OWNER_TIMEOUT when onObserverAdded runs too long, and undoes a late success', async () => {
    fake.fakeClock()
    const { sdk, socket } = worker(40)
    const slow = deferred()
    const removed = vi.fn()
    const ref = sdk
      .registerFunction('cron::on-tick', async () => null)
      .onObserverAdded(() => slow.promise)
      .onObserverRemoved(removed)
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
    sdk
      .registerFunction('cron::on-tick', async () => null)
      .onObserverAdded(async (obs) => {
        order.push(`start ${obs.id}`)
        if (obs.id === 'o-1') await first.promise
        order.push(`end ${obs.id}`)
      })
      .onObserverRemoved((obs) => {
        order.push(`removed ${obs.id}`)
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
      sdk.registerFunction(id, async () => null).onObserverAdded(async () => {
        started.push(id)
        await blocked.promise
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
    const ref = sdk.registerFunction('cron::on-tick', async () => null).onObserverAdded(added).onObserverRemoved(removed)
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
    const ref = sdk
      .registerFunction('cron::on-tick', async () => null)
      .onObserverAdded((obs) => {
        added.push(obs.id)
      })
      .onObserverRemoved((obs) => {
        removed.push(obs.id)
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
    sdk
      .registerFunction('cron::on-tick', async () => null)
      .onObserverAdded((obs) => {
        calls.push(`added ${JSON.stringify(obs.args)}`)
      })
      .onObserverRemoved((obs) => {
        calls.push(`removed ${JSON.stringify(obs.args)}`)
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
    const ref = sdk.registerFunction('orders::archive', handler).onObserverRemoved(removed)
    socket.open()
    socket.deliver(functionRegistered('orders::archive', [observerInfo('a', 't::a')]))
    await settle()
    expect(ref.isObserved()).toBe(true)
    await sdk.call({ function_id: 'orders::archive', payload: {} })
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
    void sdk.call({ function_id: 'orders::archive', payload: {}, timeoutMs: 50 }).catch(() => undefined)
    expect(socket.framesOf('invokefunction')).toHaveLength(1) // remote now
    expect(handler).toHaveBeenCalledTimes(1)
  })

  it('unregister removes the observers through onObserverRemoved', async () => {
    const { sdk, socket } = worker()
    const removed = vi.fn()
    const ref = sdk.registerFunction('orders::archive', async () => null).onObserverRemoved(removed)
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
    const ref = sdk.registerFunction('orders::archive', async () => null).onObserverRemoved(() => {
      throw new Error('cleanup failed')
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
    const ref = sdk.registerFunction('orders::archive', async () => null).onObserverAdded(added)
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
    sdk.registerFunction('cron::on-tick', async () => null).onObserverAdded((o) => {
      obs = o
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
    const ref = sdk.registerFunction('cron::on-tick', async () => null).onObserverAdded(async (obs) => {
      if (obs.id === 'sync') obs.reject('BAD_CONFIG', 'rejected inside the hook')
      if (obs.id === 'async') {
        pending.push(obs)
        await gate.promise
      }
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
    const ref = sdk
      .registerFunction('cron::on-tick', async () => null)
      .onObserverAdded(async (obs) => {
        calls.push(`added ${obs.id}`)
        if (obs.id === 'slow') await gate.promise
      })
      .onObserverRemoved((obs) => {
        calls.push(`removed ${obs.id}`)
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
})

describe('both levels: the worker hooks and the function hooks', () => {
  function recorder() {
    const calls: string[] = []
    const hook =
      (name: string, fail?: (obs: Observer) => boolean) =>
      (obs: Observer): void => {
        calls.push(`${name} ${obs.id}`)
        if (fail?.(obs)) throw new ObserveRejection('NO', `${name} refused ${obs.id}`)
      }
    return { calls, hook }
  }

  it('adds with the worker hook then the function hook, removes in reverse, for every hosted function', async () => {
    const { sdk, socket } = worker()
    const { calls, hook } = recorder()
    sdk.onObserverAdded(hook('worker+')).onObserverRemoved(hook('worker-'))
    sdk.registerFunction('a::source', (e: unknown) => e).onObserverAdded(hook('a+')).onObserverRemoved(hook('a-'))
    sdk.registerFunction('b::plain', async () => null)
    socket.open()
    socket.deliver(functionRegistered('a::source', [observerInfo('o-a', 't::a')], { revision: 1 }))
    socket.deliver(functionRegistered('b::plain', [observerInfo('o-b', 't::b')], { revision: 1 }))
    await settle()
    socket.deliver(observerRemoved('a::source', 2, 'o-a'))
    socket.deliver(observerRemoved('b::plain', 2, 'o-b'))
    await settle()
    // the hooks of different functions run concurrently: the order holds per observer
    expect(calls.filter((c) => c.endsWith('o-a'))).toEqual(['worker+ o-a', 'a+ o-a', 'a- o-a', 'worker- o-a'])
    expect(calls.filter((c) => c.endsWith('o-b'))).toEqual(['worker+ o-b', 'worker- o-b'])
  })

  it('a function hook that rejects unwinds the worker level; a worker hook that rejects runs nothing else', async () => {
    const { sdk, socket } = worker()
    const { calls, hook } = recorder()
    sdk
      .onObserverAdded(hook('worker+', (obs) => obs.id === 'worker-refuses'))
      .onObserverRemoved(hook('worker-'))
    sdk
      .registerFunction('a::source', (e: unknown) => e)
      .onObserverAdded(hook('fn+', (obs) => obs.id === 'fn-refuses'))
      .onObserverRemoved(hook('fn-'))
    socket.open()
    socket.deliver(
      functionRegistered('a::source', [observerInfo('fn-refuses', 't::a'), observerInfo('worker-refuses', 't::b')]),
    )
    await settle()
    expect(calls).toEqual(['worker+ fn-refuses', 'fn+ fn-refuses', 'worker- fn-refuses', 'worker+ worker-refuses'])
    expect(acks(socket).map((a) => [a.id, (a.error as { message?: string }).message])).toEqual([
      ['fn-refuses', 'fn+ refused fn-refuses'],
      ['worker-refuses', 'worker+ refused worker-refuses'],
    ])
    // neither was accepted: removing them runs nothing
    socket.deliver(observerRemoved('a::source', 1, 'fn-refuses'))
    await settle()
    expect(calls).toHaveLength(4)
  })

  it('runs onObserverRemoved once per accepted observer: late rejection, then its removal', async () => {
    const { sdk, socket } = worker()
    const removed = vi.fn()
    const workerRemoved = vi.fn()
    sdk.onObserverRemoved(workerRemoved)
    const ref = sdk.registerFunction('a::source', (e: unknown) => e).onObserverRemoved(removed)
    socket.open()
    socket.deliver(functionRegistered('a::source', [observerInfo('a', 't::a'), observerInfo('b', 't::b')], { revision: 1 }))
    await settle()
    const [a] = ref.observers()
    a.reject('RELOADED', 'config changed')
    a.reject('RELOADED', 'config changed')
    await settle()
    expect(removed.mock.calls.map(([obs]) => obs.id)).toEqual(['a'])
    socket.deliver(observerRemoved('a::source', 2, 'a'))
    socket.deliver(observerRemoved('a::source', 3, 'b'))
    await settle()
    expect(removed.mock.calls.map(([obs]) => obs.id)).toEqual(['a', 'b'])
    expect(workerRemoved.mock.calls.map(([obs]) => obs.id)).toEqual(['a', 'b'])
    expect(acks(socket).filter((ack) => ack.error)).toHaveLength(1)
  })

  it('a timed-out function hook unwinds the worker level, and its late success is undone by its own level', async () => {
    fake.fakeClock()
    const { sdk, socket } = worker(40)
    const slow = deferred()
    const { calls, hook } = recorder()
    sdk.onObserverAdded(hook('worker+')).onObserverRemoved(hook('worker-'))
    sdk
      .registerFunction('a::source', (e: unknown) => e)
      .onObserverAdded(async (obs) => {
        calls.push(`fn+ ${obs.id}`)
        await slow.promise
      })
      .onObserverRemoved(hook('fn-'))
    socket.open()
    socket.deliver(functionRegistered('a::source', [observerInfo('o', 't::a')]))
    await settle()
    await fake.elapse(40)
    expect(acks(socket)).toMatchObject([{ id: 'o', error: { code: 'OWNER_TIMEOUT' } }])
    expect(calls).toEqual(['worker+ o', 'fn+ o', 'worker- o'])
    slow.resolve()
    await settle()
    expect(calls).toEqual(['worker+ o', 'fn+ o', 'worker- o', 'fn- o'])
  })
})

describe('hooks by function id', () => {
  it('set before registerFunction, chained on it, or later in the same synchronous block: each runs for every observer', async () => {
    const { sdk, socket } = worker()
    const seen: string[] = []
    sdk.onObserverAdded('a::byid', (obs) => {
      seen.push(`a ${obs.id}`)
    })
    sdk.registerFunction('a::byid', (e: unknown) => e)
    sdk.registerFunction('b::chained', (e: unknown) => e).onObserverAdded((obs) => {
      seen.push(`b ${obs.id}`)
    })
    const c = sdk.registerFunction('c::later', (e: unknown) => e)
    socket.open()
    socket.deliver(functionRegistered('a::byid', [observerInfo('1', 't::a')]))
    socket.deliver(functionRegistered('b::chained', [observerInfo('2', 't::b')]))
    socket.deliver(functionRegistered('c::later', [observerInfo('3', 't::c')]))
    c.onObserverAdded((obs) => {
      seen.push(`c ${obs.id}`)
    })
    await settle()
    expect(seen.sort()).toEqual(['a 1', 'b 2', 'c 3'])
  })

  it('stay in place across unregister() and a later registration of the id; null removes one', async () => {
    const { sdk, socket } = worker()
    const added = vi.fn()
    const removed = vi.fn()
    const first = sdk.registerFunction('a::again', (e: unknown) => e).onObserverAdded(added).onObserverRemoved(removed)
    socket.open()
    socket.deliver(functionRegistered('a::again', [observerInfo('1', 't::a')]))
    await settle()
    first.unregister()
    await settle()
    expect(removed).toHaveBeenCalledTimes(1)
    const second = sdk.registerFunction('a::again', (e: unknown) => e)
    socket.deliver(functionRegistered('a::again', [observerInfo('2', 't::a')]))
    await settle()
    expect(added.mock.calls.map(([obs]) => obs.id)).toEqual(['1', '2'])
    second.onObserverAdded(null).onObserverRemoved(null)
    socket.deliver(observerAdded('a::again', 1, observerInfo('3', 't::a')))
    socket.deliver(observerRemoved('a::again', 2, '2'))
    await settle()
    expect(added).toHaveBeenCalledTimes(2)
    expect(removed).toHaveBeenCalledTimes(1)
    expect(second.observers().map((o) => o.id)).toEqual(['3'])
  })

  it('refuses a hook that is neither a function nor null', () => {
    const { sdk } = worker()
    expect(() => sdk.onObserverAdded('a::b', 'nope' as never)).toThrow(TypeError)
    expect(() => sdk.onObserverRemoved({} as never)).toThrow(TypeError)
    expect(() => sdk.onObserverDeliver('', () => null)).toThrow(TypeError)
  })

  it('warns once, when the connection is registered, for each hook id the worker has not registered', () => {
    const { sdk, socket } = worker()
    const warn = warnings()
    sdk.onObserverAdded('db::on-row-chnaged', () => undefined)
    sdk.onObserverDeliver('cron::on-tick', (call) => call.output)
    sdk.registerFunction('cron::on-tick', (e: unknown) => e)
    sdk.onObserverRemoved('gone::fn', () => undefined).onObserverRemoved('gone::fn', null)
    socket.open()
    const about = warn.mock.calls.map((c) => String(c[0])).filter((m) => m.includes('has not registered'))
    expect(about).toHaveLength(1)
    expect(about[0]).toContain('db::on-row-chnaged')
    socket.drop()
  })
})

describe('catch-up: a hook set while observers are active', () => {
  it('runs a late function hook for each active observer, warns once with the count, late-rejects on a throw', async () => {
    const { sdk, socket } = worker()
    const warn = warnings()
    const removed = vi.fn()
    const ref = sdk.registerFunction('a::source', (e: unknown) => e)
    socket.open()
    socket.deliver(
      functionRegistered('a::source', [observerInfo('ok', 't::a'), observerInfo('bad', 't::b')], { revision: 3 }),
    )
    await settle()
    expect(ref.observers()).toHaveLength(2)
    const added = vi.fn((obs: Observer) => {
      if (obs.id === 'bad') throw new ObserveRejection('BAD_ARGS', 'no table')
    })
    ref.onObserverAdded(added).onObserverRemoved(removed)
    await settle()
    expect(added.mock.calls.map(([obs]) => obs.id)).toEqual(['ok', 'bad'])
    const caughtUp = warn.mock.calls.map((c) => String(c[0])).filter((m) => m.includes('were active without it'))
    expect(caughtUp).toHaveLength(1)
    expect(caughtUp[0]).toContain('a::source while 2 observer(s)')
    expect(acks(socket).filter((a) => a.error)).toEqual([
      expect.objectContaining({ id: 'bad', revision: 3, error: { code: 'BAD_ARGS', message: 'no table' } }),
    ])
    expect(ref.observers().map((o) => o.id)).toEqual(['ok'])
    // the function level of `bad` never resolved: nothing to tear down there
    expect(removed).not.toHaveBeenCalled()
    // setting it again runs nothing new: every active observer has run one
    ref.onObserverAdded(added)
    await settle()
    expect(added).toHaveBeenCalledTimes(2)
  })

  it('runs a late worker hook for the active observers of every hosted function', async () => {
    const { sdk, socket } = worker()
    const warn = warnings()
    sdk.registerFunction('a::one', (e: unknown) => e)
    sdk.registerFunction('b::two', (e: unknown) => e)
    socket.open()
    socket.deliver(functionRegistered('a::one', [observerInfo('1', 't::a')]))
    socket.deliver(functionRegistered('b::two', [observerInfo('2', 't::b'), observerInfo('3', 't::b')]))
    await settle()
    const added = vi.fn()
    sdk.onObserverAdded(added)
    await settle()
    expect(added.mock.calls.map(([obs]) => obs.id).sort()).toEqual(['1', '2', '3'])
    expect(warn.mock.calls.map((c) => String(c[0])).filter((m) => m.includes('were active without it'))).toHaveLength(2)
  })
})

describe('registrations that need observe.v2 (decisions 18, 41)', () => {
  const v2Functions = (sdk: IIIClient) => {
    sdk.registerFunction('a::plain', async () => null)
    sdk.registerFunction('b::args', (e: unknown) => e, { args_format: { type: 'object' } })
    sdk.registerFunction('c::private', (e: unknown) => e, { private: true })
    sdk.registerFunction('d::secret', async () => null, { observable: false })
    sdk.registerFunction('e::routed', async () => null).onObserverDeliver((call) => call.output)
  }

  it('wait for WorkerRegistered, then go out when the engine advertises observe.v2', () => {
    const { sdk, socket } = worker()
    v2Functions(sdk)
    socket.open(null)
    expect(socket.framesOf('registerfunction').map((f) => f.id)).toEqual(['a::plain'])
    socket.deliver({ type: 'workerregistered', worker_id: 'w-1', features: fake.ENGINE_FEATURES })
    expect(socket.framesOf('registerfunction').map((f) => f.id)).toEqual([
      'a::plain',
      'b::args',
      'c::private',
      'd::secret',
      'e::routed',
    ])
  })

  it('stay unregistered against an engine without it, logging UNSUPPORTED once per function; decided again on reconnect', async () => {
    vi.useFakeTimers()
    const { sdk, socket } = worker()
    const warn = warnings()
    v2Functions(sdk)
    socket.open(fake.ENGINE_FEATURES_V1)
    sdk.registerFunction('f::late', (e: unknown) => e, { private: true })
    expect(socket.framesOf('registerfunction').map((f) => f.id)).toEqual(['a::plain'])
    const unsupported = () => warn.mock.calls.map((c) => String(c[0])).filter((m) => m.startsWith('[iii] UNSUPPORTED'))
    expect(unsupported().map((m) => m.match(/"([^"]+)"/)?.[1])).toEqual([
      'b::args',
      'c::private',
      'd::secret',
      'e::routed',
      'f::late',
    ])

    socket.drop()
    await vi.advanceTimersByTimeAsync(50)
    const again = fake.lastSocket()
    again.open(fake.ENGINE_FEATURES_V1)
    expect(again.framesOf('registerfunction').map((f) => f.id)).toEqual(['a::plain'])
    expect(unsupported()).toHaveLength(5) // once per function

    again.drop()
    await vi.advanceTimersByTimeAsync(50)
    const upgraded = fake.lastSocket()
    upgraded.open()
    expect(upgraded.framesOf('registerfunction').map((f) => f.id)).toEqual([
      'a::plain',
      'b::args',
      'c::private',
      'd::secret',
      'e::routed',
      'f::late',
    ])
  })

  it('a deliver hook set later on a function registered with such an engine unregisters it; removing it registers it again', () => {
    const { sdk, socket } = worker()
    warnings()
    const ref = sdk.registerFunction('a::plain', async () => null)
    socket.open(fake.ENGINE_FEATURES_V1)
    socket.clear()
    ref.onObserverDeliver((call) => call.output)
    expect(socket.frames().map((f) => [f.type, f.id])).toEqual([['unregisterfunction', 'a::plain']])
    ref.onObserverDeliver(null)
    expect(socket.frames().map((f) => [f.type, f.id])).toEqual([
      ['unregisterfunction', 'a::plain'],
      ['registerfunction', 'a::plain'],
    ])
  })

  it('INVALID_ARGS_FORMAT and UNSUPPORTED refuse one function and keep the connection', async () => {
    const { sdk, socket } = worker()
    const errors = vi.spyOn(console, 'error').mockImplementation(() => undefined)
    sdk.registerFunction('queue::on-message', (m: unknown) => m, { args_format: { type: 'nonsense' } })
    socket.open()
    socket.deliver(fake.functionRegistered('queue::on-message', [observerInfo('o', 't::a')]))
    await settle()
    for (const code of ['INVALID_ARGS_FORMAT', 'UNSUPPORTED']) {
      socket.deliver({
        type: 'registrationrejected',
        code,
        namespace: 'default',
        function_id: 'queue::on-message',
        owner_worker_id: '',
      })
    }
    await settle()
    expect(sdk.getConnectionState()).toBe('connected')
    expect(sdk.getFatalError()).toBeUndefined()
    expect(sdk.isObserved('queue::on-message')).toBe(false)
    expect(errors.mock.calls.map((c) => String(c[0])).filter((m) => m.includes('INVALID_ARGS_FORMAT'))).toHaveLength(1)
  })
})

describe('standby (decision 21)', () => {
  const conflict = (fn: string) => ({
    type: 'registrationrejected',
    code: 'FUNCTION_NAMESPACE_CONFLICT',
    namespace: 'default',
    function_id: fn,
    owner_worker_id: 'host-1',
  })

  it('retries the registration every second (±20 %) until the engine accepts it; then the hooks run', async () => {
    fake.fakeClock()
    const { sdk, socket } = worker()
    vi.spyOn(console, 'info').mockImplementation(() => undefined)
    const added = vi.fn()
    sdk.registerFunction('cron::on-tick', (t: unknown) => t, { private: true, standby: true }).onObserverAdded(added)
    socket.open()
    socket.deliver(conflict('cron::on-tick'))
    socket.clear()
    await fake.elapse(799)
    expect(socket.framesOf('registerfunction')).toHaveLength(0)
    await fake.elapse(401)
    expect(socket.framesOf('registerfunction')).toHaveLength(1)
    socket.deliver(conflict('cron::on-tick'))
    await fake.elapse(1200)
    expect(socket.framesOf('registerfunction')).toHaveLength(2)
    socket.deliver(functionRegistered('cron::on-tick', [observerInfo('a', 't::a'), observerInfo('b', 't::b')]))
    await settle()
    expect(added).toHaveBeenCalledTimes(2)
    await fake.elapse(5000)
    expect(socket.framesOf('registerfunction')).toHaveLength(2)
  })

  it('stops when the function is unregistered; a function without standby only logs the conflict', async () => {
    fake.fakeClock()
    const { sdk, socket } = worker()
    vi.spyOn(console, 'info').mockImplementation(() => undefined)
    const warn = warnings()
    const ref = sdk.registerFunction('cron::on-tick', (t: unknown) => t, { standby: true })
    sdk.registerFunction('other::fn', async () => null)
    socket.open()
    socket.deliver(conflict('cron::on-tick'))
    socket.deliver(conflict('other::fn'))
    ref.unregister()
    socket.clear()
    await fake.elapse(5000)
    expect(socket.framesOf('registerfunction')).toEqual([])
    expect(warn.mock.calls.map((c) => String(c[0])).filter((m) => m.includes('already exported'))).toHaveLength(1)
  })
})

describe('an engine without observe (no observe.v1)', () => {
  it('registers a plain function and logs once that its observer hooks are inactive', () => {
    const { sdk, socket } = worker()
    const warn = warnings()
    sdk.registerFunction('cron::on-tick', async () => null).onObserverAdded(() => undefined)
    sdk.registerFunction('plain::fn', async () => null)
    socket.open('legacy')
    sdk.onObserverRemoved('db::on-row', () => undefined)
    sdk.registerFunction('db::on-row', async () => null)
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
    const state = new FunctionObservers('f::boom', true, true, {
      sendAck: () => {
        throw new Error('ack failed')
      },
      reporter,
      hooks: () => ({}),
      logError: (message) => errors.push(message),
      logWarn: () => undefined,
      hookTimeoutMs: () => 30_000,
      observeV2: () => true,
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
    await sdk.call({ function_id: 'f::twice', payload: 1 })
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
