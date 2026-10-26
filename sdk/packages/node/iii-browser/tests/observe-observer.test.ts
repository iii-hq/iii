// Derived from sdk/packages/node/iii/tests/observe-observer.test.ts by scripts/sync-browser-observe.py; edit those, not this file.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ObserveRejection } from '../src/errors'
import { registerWorker } from '../src/index'
import type { ISdk } from '../src/types'
import * as fake from './observe-fake-engine'

/** `iii.observe` against a fake engine: the handle, its states, replay and dispose (sdk.md §2, §4.5). */
let sdk: ISdk | undefined
afterEach(async () => {
  vi.useRealTimers()
  await sdk?.shutdown()
  sdk = undefined
})

function worker() {
  sdk = registerWorker('ws://observe.test', {
    reconnectionConfig: { initialDelayMs: 10, jitterFactor: 0 },
  })
  return { sdk, socket: fake.lastSocket() }
}

describe('iii.observe', () => {
  it('returns a pending handle synchronously, before the connection is up, and sends it after WorkerRegistered', () => {
    const { sdk, socket } = worker()
    const handle = sdk.observe({ function_id: 'orders::create' }, 'audit::record')
    expect(handle.state).toBe('pending')
    expect(handle.id).toMatch(/^[0-9a-f-]{36}$/)
    expect(socket.sent).toHaveLength(0)

    socket.open(null) // open, no WorkerRegistered yet: the engine's features are unknown
    expect(socket.framesOf('observe')).toHaveLength(0)
    socket.deliver({ type: 'workerregistered', worker_id: 'w-1', features: fake.ENGINE_FEATURES })
    expect(socket.framesOf('observe')).toEqual([
      {
        type: 'observe',
        id: handle.id,
        source: { function_id: 'orders::create' },
        target: { function_id: 'audit::record' },
      },
    ])
  })

  it('sends the functions before the observers on every connection', () => {
    const { sdk, socket } = worker()
    sdk.registerFunction('audit::record', async () => null)
    sdk.observe({ function_id: 'orders::create' }, 'audit::record')
    socket.open()
    const types = socket.frames().map((f) => f.type)
    expect(types.indexOf('registerfunction')).toBeLessThan(types.indexOf('observe'))
  })

  it('fills the namespace from the worker, and `default` for engine builtins', () => {
    sdk = registerWorker('ws://observe.test', {
      namespace: 'orders',
    })
    const socket = fake.lastSocket()
    socket.open()
    sdk.observe({ function_id: 'orders::create' }, 'audit::record')
    sdk.observe({ function_id: 'engine::on-functions-changed' }, { function_id: 'audit::record', namespace: 'ops' })
    const [a, b] = socket.framesOf('observe')
    expect(a.source).toEqual({ function_id: 'orders::create', namespace: 'orders' })
    expect(a.target).toEqual({ function_id: 'audit::record', namespace: 'orders' })
    expect(b.source).toEqual({ function_id: 'engine::on-functions-changed', namespace: 'default' })
    expect(b.target).toEqual({ function_id: 'audit::record', namespace: 'ops' })
  })

  it('follows observeresult and observerstatechanged; ready() resolves on active', async () => {
    const { sdk, socket } = worker()
    socket.open()
    const handle = sdk.observe({ function_id: 'orders::create' }, 'audit::record')
    const states: string[] = []
    handle.onStateChange((s) => states.push(s))
    const ready = handle.ready()
    socket.deliver({ type: 'observeresult', id: handle.id, state: 'pending' })
    socket.deliver({ type: 'observerstatechanged', id: handle.id, state: 'active', fires: 0 })
    await expect(ready).resolves.toBeUndefined()
    expect(handle.state).toBe('active')
    expect(states).toEqual(['active'])
    await expect(handle.ready()).resolves.toBeUndefined()

    socket.deliver({
      type: 'observerstatechanged',
      id: handle.id,
      state: 'suspended',
      reason: { code: 'TARGET_UNAVAILABLE', message: 'gone' },
      fires: 2,
    })
    expect(handle.state).toBe('suspended')
    expect(handle.reason).toEqual({ code: 'TARGET_UNAVAILABLE', message: 'gone' })
  })

  it('rejects ready() with the code when the engine answers error', async () => {
    const { sdk, socket } = worker()
    socket.open()
    const handle = sdk.observe({ function_id: 'cron::on-tick', args: {} }, 'audit::record')
    const ready = handle.ready()
    socket.deliver({
      type: 'observeresult',
      id: handle.id,
      state: 'error',
      error: { code: 'INVALID_ARGS', message: 'expression is required' },
    })
    await expect(ready).rejects.toBeInstanceOf(ObserveRejection)
    await expect(handle.ready()).rejects.toMatchObject({ code: 'INVALID_ARGS', message: 'expression is required' })
    expect(handle.state).toBe('error')
  })

  it('goes to error with UNSUPPORTED against an engine without observe.v1, and is not sent', async () => {
    const { sdk, socket } = worker()
    const before = sdk.observe({ function_id: 'orders::create' }, 'audit::record')
    socket.open('legacy') // an old engine: WorkerRegistered without features
    await expect(before.ready()).rejects.toMatchObject({ code: 'UNSUPPORTED' })
    const after = sdk.observe({ function_id: 'orders::create' }, 'audit::record')
    expect(after.state).toBe('error')
    expect(after.reason?.code).toBe('UNSUPPORTED')
    expect(socket.framesOf('observe')).toHaveLength(0)
  })

  it('replays with the same id after a reconnect; a disposed observer is not replayed', async () => {
    vi.useFakeTimers()
    const { sdk, socket } = worker()
    socket.open()
    const kept = sdk.observe({ function_id: 'orders::create' }, 'audit::record', { id: 'kept-1' })
    const gone = sdk.observe({ function_id: 'orders::create' }, 'audit::record', { id: 'gone-1' })
    await gone.dispose()
    expect(gone.state).toBe('disposed')
    expect(socket.framesOf('unobserve')).toEqual([{ type: 'unobserve', id: 'gone-1' }])

    socket.drop()
    await vi.advanceTimersByTimeAsync(50)
    const next = fake.lastSocket()
    expect(next).not.toBe(socket)
    next.open()
    expect(next.framesOf('observe').map((f) => f.id)).toEqual([kept.id])
    expect(next.framesOf('observe')[0]).toEqual(socket.framesOf('observe')[0])
  })

  it('queues the unobserve of an observer disposed while disconnected', async () => {
    vi.useFakeTimers()
    const { sdk, socket } = worker()
    socket.open()
    const handle = sdk.observe({ function_id: 'orders::create' }, 'audit::record')
    socket.drop()
    await handle.dispose()
    await vi.advanceTimersByTimeAsync(50)
    const next = fake.lastSocket()
    next.open()
    expect(next.framesOf('unobserve')).toEqual([{ type: 'unobserve', id: handle.id }])
    expect(next.framesOf('observe')).toHaveLength(0)
  })

  it('drops a queued unobserve when the next engine does not support observe', async () => {
    vi.useFakeTimers()
    const { sdk, socket } = worker()
    socket.open()
    const handle = sdk.observe({ function_id: 'orders::create' }, 'audit::record')
    socket.drop()
    await handle.dispose()
    await vi.advanceTimersByTimeAsync(50)
    const next = fake.lastSocket()
    next.open(null) // open: the queued frame waits for WorkerRegistered
    expect(next.framesOf('unobserve')).toHaveLength(0)
    next.deliver({ type: 'workerregistered', worker_id: 'w-2' }) // a pre-0.25 engine
    expect(next.framesOf('unobserve')).toHaveLength(0)
  })

  it('a source that is not JSON turns the handle to error (INVALID_ARGS) and sends nothing', async () => {
    const { sdk, socket } = worker()
    socket.open()
    const cyclic: Record<string, unknown> = {}
    cyclic.self = cyclic
    const big = sdk.observe({ function_id: 'cron::on-tick', args: { n: 1n } }, 'audit::record')
    const loop = sdk.observe({ function_id: 'cron::on-tick', args: cyclic }, 'audit::record')
    for (const handle of [big, loop]) {
      expect(handle.state).toBe('error')
      await expect(handle.ready()).rejects.toMatchObject({ code: 'INVALID_ARGS' })
    }
    expect(socket.framesOf('observe')).toHaveLength(0)
    await big.dispose()
    expect(socket.framesOf('unobserve')).toHaveLength(0)
  })

  it.each([
    ['an undefined operand', { 'input.a': { $eq: undefined } }, 'input.a.$eq'],
    ['NaN', { 'input.a': { $gt: Number.NaN } }, 'input.a.$gt'],
    ['a Date', { 'input.at': { $gte: new Date(0) } }, 'input.at.$gte'],
    ['undefined in an array', { 'input.a': { $in: [1, undefined] } }, 'input.a.$in[1]'],
    ['a Map', new Map([['input.a', { $eq: 1 }]]), 'the filter'],
    ['a function', { 'input.a': { $eq: () => 1 } }, 'input.a.$eq'],
    ['a literal shorthand', { 'input.a': 1 }, 'input.a'],
    [
      'a cycle',
      (() => {
        const c: Record<string, unknown> = {}
        c['input.a'] = { $eq: c }
        return c
      })(),
      '`input.a.$eq`: a cycle',
    ],
  ] as const)('filters with %s: error INVALID_FILTER naming where, nothing sent', async (_name, filters, where) => {
    const { sdk, socket } = worker()
    socket.open()
    const handle = sdk.observe({ function_id: 'orders::create', filters: filters as never }, 'audit::record')
    expect(handle.state).toBe('error')
    expect(handle.reason?.code).toBe('INVALID_FILTER')
    expect(handle.reason?.message).toContain(where)
    await expect(handle.ready()).rejects.toMatchObject({ code: 'INVALID_FILTER' })
    expect(socket.framesOf('observe')).toHaveLength(0)
  })

  it('forgets an observer the engine disposed (max_fires, expiry)', async () => {
    vi.useFakeTimers()
    const { sdk, socket } = worker()
    socket.open()
    const handle = sdk.observe({ function_id: 'orders::create' }, 'audit::record', { max_fires: 1 })
    socket.deliver({
      type: 'observerstatechanged',
      id: handle.id,
      state: 'disposed',
      reason: { code: 'MAX_FIRES', message: 'max_fires reached' },
      fires: 1,
    })
    await expect(handle.ready()).rejects.toMatchObject({ code: 'MAX_FIRES' })
    socket.drop()
    await vi.advanceTimersByTimeAsync(50)
    const next = fake.lastSocket()
    next.open()
    expect(next.framesOf('observe')).toHaveLength(0)
  })

  it('refuses a reused id and a missing source or target', () => {
    const { sdk } = worker()
    sdk.observe({ function_id: 'a::b' }, 'c::d', { id: 'same' })
    expect(() => sdk.observe({ function_id: 'a::b' }, 'c::d', { id: 'same' })).toThrow(/already in use/)
    expect(() => sdk.observe({ function_id: '' }, 'c::d')).toThrow(/function_id/)
    expect(() => sdk.observe({ function_id: 'a::b' }, '')).toThrow(/target/)
  })

  it('a state listener that throws does not stop the others', async () => {
    const { sdk, socket } = worker()
    socket.open()
    const handle = sdk.observe({ function_id: 'orders::create' }, 'audit::record')
    const seen: string[] = []
    handle.onStateChange(() => {
      throw new Error('boom')
    })
    const off = handle.onStateChange((s) => seen.push(s))
    socket.deliver({ type: 'observerstatechanged', id: handle.id, state: 'active', fires: 0 })
    await fake.settle()
    off()
    socket.deliver({ type: 'observerstatechanged', id: handle.id, state: 'suspended', fires: 0 })
    await fake.settle()
    expect(seen).toEqual(['active'])
  })

  it('state listeners run after the read path returns, in order, and may call the client', async () => {
    const { sdk, socket } = worker()
    socket.open()
    const handle = sdk.observe({ function_id: 'orders::create' }, 'audit::record')
    const seen: string[] = []
    let inside = false
    handle.onStateChange((state, reason) => {
      expect(inside).toBe(false) // not called from within the frame handling
      seen.push(`${state}${reason ? `:${reason.code}` : ''}`)
      if (state === 'error') void handle.dispose() // calling the client from a listener is fine
    })
    inside = true
    socket.deliver({ type: 'observerstatechanged', id: handle.id, state: 'active', fires: 0 })
    socket.deliver({
      type: 'observerstatechanged',
      id: handle.id,
      state: 'suspended',
      reason: { code: 'TARGET_UNAVAILABLE', message: 'gone' },
      fires: 0,
    })
    socket.deliver({
      type: 'observerstatechanged',
      id: handle.id,
      state: 'error',
      reason: { code: 'OWNER_TIMEOUT', message: 'late' },
      fires: 0,
    })
    inside = false
    expect(seen).toEqual([])
    await fake.settle()
    expect(seen).toEqual(['active', 'suspended:TARGET_UNAVAILABLE', 'error:OWNER_TIMEOUT', 'disposed:UNOBSERVED'])
    expect(socket.framesOf('unobserve')).toEqual([{ type: 'unobserve', id: handle.id }])
  })
})
