import { afterEach, describe, expect, it, vi } from 'vitest'
import type { IIIClient } from '../src/types'

/** `iii.observe` against a fake engine: the handle, its states, replay and dispose (sdk.md §2, §4.5). */
vi.mock('ws', async () => (await import('./observe-fake-engine')).wsModule())
vi.resetModules()
const fake = await import('./observe-fake-engine')
const { registerWorker } = await import('../src/index')
const { ObserveRejection } = await import('../src/errors')

let sdk: IIIClient | undefined
afterEach(async () => {
  vi.useRealTimers()
  await sdk?.shutdown()
  sdk = undefined
})

function worker() {
  sdk = registerWorker('ws://observe.test', {
    otel: { enabled: false },
    enableMetricsReporting: false,
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

  it('sends the functions before the observers on every connection, those that wait for observe.v2 included', () => {
    const { sdk, socket } = worker()
    sdk.registerFunction('audit::record', async () => null, { private: true })
    sdk.observe({ function_id: 'orders::create' }, 'audit::record')
    socket.open()
    const types = socket.frames().map((f) => f.type)
    expect(types.indexOf('registerfunction')).toBeGreaterThan(-1)
    expect(types.indexOf('registerfunction')).toBeLessThan(types.indexOf('observe'))
  })

  it("observe('fn::id', target) is observe({ function_id: 'fn::id' }, target)", () => {
    const { sdk, socket } = worker()
    socket.open()
    sdk.observe('orders::create', 'audit::record', { id: 'short' })
    sdk.observe({ function_id: 'orders::create' }, 'audit::record', { id: 'long' })
    const [short, long] = socket.framesOf('observe')
    expect({ ...short, id: 'long' }).toEqual(long)
    expect(short.source).toEqual({ function_id: 'orders::create' })
  })

  it('fills the namespace from the worker, and `default` for engine builtins', () => {
    sdk = registerWorker('ws://observe.test', {
      otel: { enabled: false },
      enableMetricsReporting: false,
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
    socket.deliver({ type: 'observeresult', id: handle.id, state: 'active' })
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
    expect(() => sdk.observe({ function_id: 'a::b' }, 'c::d', { id: 'same' })).toThrow(ObserveRejection)
    expect(() => sdk.observe({ function_id: '' }, 'c::d')).toThrow(/function_id/)
    expect(() => sdk.observe('', 'c::d')).toThrow(/function_id/)
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
    socket.deliver({ type: 'observeresult', id: handle.id, state: 'pending' })
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
    socket.deliver({ type: 'observeresult', id: handle.id, state: 'pending' })
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

describe('observer ids (decision 56)', () => {
  it('refuses an id a handle holds, in any state, with OBSERVER_ID_CONFLICT: nothing sent, the holder untouched', async () => {
    const { sdk, socket } = worker()
    socket.open()
    const holder = sdk.observe({ function_id: 'orders::create' }, 'audit::record', { id: 'held' })
    socket.deliver({ type: 'observeresult', id: 'held', state: 'active' })
    for (const state of ['active', 'suspended', 'error'] as const) {
      if (state !== 'active') socket.deliver({ type: 'observerstatechanged', id: 'held', state, fires: 0 })
      let thrown: unknown
      try {
        sdk.observe({ function_id: 'orders::create' }, 'audit::record', { id: 'held' })
      } catch (error) {
        thrown = error
      }
      expect(thrown).toBeInstanceOf(ObserveRejection)
      expect(thrown).toMatchObject({
        code: 'OBSERVER_ID_CONFLICT',
        message: 'observe: observer id already in use: held',
      })
      expect(holder.state).toBe(state)
    }
    expect(socket.framesOf('observe')).toHaveLength(1)
    expect(socket.framesOf('unobserve')).toHaveLength(0)
    await holder.dispose()
    expect(socket.framesOf('unobserve')).toEqual([{ type: 'unobserve', id: 'held' }])
  })

  it('an id is free once disposed: the earlier observer’s late disposed reaches the earlier handle only', async () => {
    const { sdk, socket } = worker()
    socket.open()
    const first = sdk.observe({ function_id: 'orders::create' }, 'audit::record', { id: 'q' })
    socket.deliver({ type: 'observeresult', id: 'q', state: 'active' })
    await first.dispose()
    const second = sdk.observe({ function_id: 'orders::create' }, 'audit::record', { id: 'q' })
    expect(
      socket
        .frames()
        .map((f) => f.type)
        .filter((t) => t === 'observe' || t === 'unobserve'),
    ).toEqual(['observe', 'unobserve', 'observe'])
    // The engine read unobserve before the new observe: its disposed comes first.
    socket.deliver({
      type: 'observerstatechanged',
      id: 'q',
      state: 'disposed',
      reason: { code: 'UNOBSERVED', message: 'unobserved' },
      fires: 0,
    })
    expect(second.state).toBe('pending')
    socket.deliver({ type: 'observeresult', id: 'q', state: 'active' })
    await expect(second.ready()).resolves.toBeUndefined()
    expect(second.state).toBe('active')
    expect(first.reason?.code).toBe('UNOBSERVED')
    socket.deliver({ type: 'observerstatechanged', id: 'q', state: 'suspended', fires: 0 })
    expect(second.state).toBe('suspended')
    await second.dispose()
    expect(socket.framesOf('unobserve')).toEqual([
      { type: 'unobserve', id: 'q' },
      { type: 'unobserve', id: 'q' },
    ])
  })

  it('an observeresult answers the oldest unanswered observe of the id: one disposed before its answer', async () => {
    const { sdk, socket } = worker()
    socket.open()
    const first = sdk.observe({ function_id: 'orders::create' }, 'audit::record', { id: 'q' })
    await first.dispose()
    const second = sdk.observe({ function_id: 'orders::create' }, 'audit::record', { id: 'q' })
    // first's answer, first's disposed, then second's answer
    socket.deliver({ type: 'observeresult', id: 'q', state: 'active' })
    expect(second.state).toBe('pending')
    socket.deliver({ type: 'observerstatechanged', id: 'q', state: 'disposed', fires: 0 })
    expect(second.state).toBe('pending')
    socket.deliver({
      type: 'observeresult',
      id: 'q',
      state: 'error',
      error: { code: 'FORBIDDEN', message: 'no' },
    })
    expect(second.state).toBe('error')
    expect(second.reason?.code).toBe('FORBIDDEN')
    expect(first.state).toBe('disposed')
    expect(first.reason?.code).toBe('UNOBSERVED')
  })

  it('drops an observerstatechanged that arrives before any observeresult of the id', async () => {
    vi.useFakeTimers()
    const { sdk, socket } = worker()
    socket.open()
    const handle = sdk.observe({ function_id: 'orders::create' }, 'audit::record', { id: 'r' })
    socket.deliver({ type: 'observeresult', id: 'r', state: 'active' })
    socket.drop()
    await vi.advanceTimersByTimeAsync(50)
    const next = fake.lastSocket()
    next.open()
    // A reattached connection hears the observer resume before the replay's answer.
    next.deliver({ type: 'observerstatechanged', id: 'r', state: 'disposed', fires: 0 })
    expect(handle.state).toBe('active')
    next.deliver({ type: 'observeresult', id: 'r', state: 'suspended' })
    expect(handle.state).toBe('suspended')
  })

  it('on a new connection, the unobserve of a disposed handle goes out before the replay of its id', async () => {
    vi.useFakeTimers()
    const { sdk, socket } = worker()
    socket.open()
    const first = sdk.observe({ function_id: 'orders::create' }, 'audit::record', { id: 'q' })
    socket.deliver({ type: 'observeresult', id: 'q', state: 'active' })
    socket.drop()
    await first.dispose()
    const second = sdk.observe({ function_id: 'orders::create' }, 'audit::record', { id: 'q' })
    await vi.advanceTimersByTimeAsync(50)
    const next = fake.lastSocket()
    next.open()
    expect(
      next
        .frames()
        .filter((f) => f.type === 'observe' || f.type === 'unobserve')
        .map((f) => `${f.type}:${f.id}`),
    ).toEqual(['unobserve:q', 'observe:q'])
    // The reattached earlier observer's disposed precedes the replay's answer.
    next.deliver({ type: 'observerstatechanged', id: 'q', state: 'disposed', fires: 0 })
    next.deliver({ type: 'observeresult', id: 'q', state: 'active' })
    expect(second.state).toBe('active')
  })

  it('an observer the engine disposed frees its id; its handle sends no unobserve', async () => {
    const { sdk, socket } = worker()
    socket.open()
    const first = sdk.observe({ function_id: 'orders::create' }, 'audit::record', { id: 'm', max_fires: 1 })
    socket.deliver({ type: 'observeresult', id: 'm', state: 'active' })
    socket.deliver({
      type: 'observerstatechanged',
      id: 'm',
      state: 'disposed',
      reason: { code: 'MAX_FIRES', message: 'max_fires reached' },
      fires: 1,
    })
    const second = sdk.observe({ function_id: 'orders::create' }, 'audit::record', { id: 'm' })
    await first.dispose()
    expect(socket.framesOf('unobserve')).toHaveLength(0)
    socket.deliver({ type: 'observeresult', id: 'm', state: 'active' })
    expect(second.state).toBe('active')
  })
})

describe('a refused observe', () => {
  // Rust's dispose() waits up to 5 s for the engine's answer; this SDK's never waits for it.
  const DISPOSE_WAIT_MS = 5000
  async function timedDispose(handle: { dispose(): Promise<void> }): Promise<number> {
    const started = performance.now()
    await handle.dispose()
    return performance.now() - started
  }
  const observeFrames = (socket: ReturnType<typeof fake.lastSocket>) =>
    socket
      .frames()
      .filter((f) => f.type === 'observe' || f.type === 'unobserve')
      .map((f) => `${f.type}:${f.id}`)

  it('is the handle’s final answer: its dispose() sends nothing and resolves at once', async () => {
    const { sdk, socket } = worker()
    socket.open()
    const handle = sdk.observe({ function_id: 'orders::create' }, 'audit::record', { id: 'r' })
    const ready = handle.ready()
    socket.deliver({
      type: 'observeresult',
      id: 'r',
      state: 'error',
      error: { code: 'FORBIDDEN', message: 'no' },
    })
    await expect(ready).rejects.toMatchObject({ code: 'FORBIDDEN', message: 'no' })
    // Out of the id's list: nothing the engine says about the id reaches it.
    socket.deliver({ type: 'observerstatechanged', id: 'r', state: 'active', fires: 0 })
    expect(handle.state).toBe('error')
    expect(await timedDispose(handle)).toBeLessThan(DISPOSE_WAIT_MS / 10)
    expect(handle.state).toBe('disposed')
    expect(observeFrames(socket)).toEqual(['observe:r'])
  })

  it('a dispose sent before the refusal resolves at once; the refusal finds it and the id is free', async () => {
    const { sdk, socket } = worker()
    socket.open()
    const handle = sdk.observe({ function_id: 'orders::create' }, 'audit::record', { id: 'r' })
    expect(await timedDispose(handle)).toBeLessThan(DISPOSE_WAIT_MS / 10)
    expect(observeFrames(socket)).toEqual(['observe:r', 'unobserve:r'])
    socket.deliver({
      type: 'observeresult',
      id: 'r',
      state: 'error',
      error: { code: 'INVALID_ARGS', message: 'bad args' },
    })
    expect(handle.state).toBe('disposed')
    expect(handle.reason?.code).toBe('UNOBSERVED')
    // The engine ignores that unobserve and answers nothing for it: the next
    // observe of the id hears its own answer.
    const next = sdk.observe({ function_id: 'orders::create' }, 'audit::record', { id: 'r' })
    socket.deliver({ type: 'observeresult', id: 'r', state: 'active' })
    expect(next.state).toBe('active')
    socket.deliver({ type: 'observerstatechanged', id: 'r', state: 'suspended', fires: 0 })
    expect(next.state).toBe('suspended')
    await next.dispose()
    expect(observeFrames(socket)).toEqual(['observe:r', 'unobserve:r', 'observe:r', 'unobserve:r'])
  })

  it('the refused handle holds its id until disposed; the id observed again after works normally', async () => {
    const { sdk, socket } = worker()
    socket.open()
    const refused = sdk.observe({ function_id: 'orders::create' }, 'audit::record', { id: 'r' })
    socket.deliver({
      type: 'observeresult',
      id: 'r',
      state: 'error',
      error: {
        code: 'OBSERVER_ID_CONFLICT',
        message: 'observer `r` is owned by another connection',
      },
    })
    expect(() => sdk.observe({ function_id: 'orders::create' }, 'audit::record', { id: 'r' })).toThrow(
      'observe: observer id already in use: r',
    )
    await refused.dispose()
    const next = sdk.observe({ function_id: 'orders::create' }, 'audit::record', { id: 'r' })
    expect(observeFrames(socket)).toEqual(['observe:r', 'observe:r'])
    socket.deliver({ type: 'observeresult', id: 'r', state: 'active' })
    await expect(next.ready()).resolves.toBeUndefined()
    expect(refused.reason?.code).toBe('UNOBSERVED')
    await next.dispose()
    expect(observeFrames(socket)).toEqual(['observe:r', 'observe:r', 'unobserve:r'])
  })

  it('a refusal for an older handle of the id leaves the newer, pending one to its own answer', async () => {
    const { sdk, socket } = worker()
    socket.open()
    const older = sdk.observe({ function_id: 'orders::create' }, 'audit::record', { id: 'r' })
    await older.dispose()
    const newer = sdk.observe({ function_id: 'orders::create' }, 'audit::record', { id: 'r' })
    socket.deliver({
      type: 'observeresult',
      id: 'r',
      state: 'error',
      error: { code: 'FORBIDDEN', message: 'no' },
    })
    expect(older.state).toBe('disposed')
    expect(older.reason?.code).toBe('UNOBSERVED')
    expect(newer.state).toBe('pending')
    // Nothing is listed with an answer yet: a state change before the newer one's answer is dropped.
    socket.deliver({ type: 'observerstatechanged', id: 'r', state: 'error', fires: 0 })
    expect(newer.state).toBe('pending')
    socket.deliver({ type: 'observeresult', id: 'r', state: 'active' })
    expect(newer.state).toBe('active')
    expect(await timedDispose(newer)).toBeLessThan(DISPOSE_WAIT_MS / 10)
    expect(observeFrames(socket)).toEqual(['observe:r', 'unobserve:r', 'observe:r', 'unobserve:r'])
  })

  it('a refused handle is not sent again on a later connection: it keeps its refusal and its id until dispose(), which sends nothing', async () => {
    vi.useFakeTimers()
    const { sdk, socket } = worker()
    socket.open()
    const handle = sdk.observe({ function_id: 'orders::create' }, 'audit::record', { id: 'r' })
    const states: string[] = []
    handle.onStateChange((state) => states.push(state))
    socket.deliver({
      type: 'observeresult',
      id: 'r',
      state: 'error',
      error: { code: 'OBSERVER_ID_CONFLICT', message: 'taken' },
    })
    socket.drop()
    await vi.advanceTimersByTimeAsync(50)
    const next = fake.lastSocket()
    next.open()
    expect(observeFrames(next)).toEqual([])
    // Nothing the engine says about the id reaches it.
    next.deliver({ type: 'observerstatechanged', id: 'r', state: 'active', fires: 0 })
    expect(handle.state).toBe('error')
    expect(handle.reason?.code).toBe('OBSERVER_ID_CONFLICT')
    // An engine without observe.v1 on the connection after: the refusal stays its reason.
    next.drop()
    await vi.advanceTimersByTimeAsync(50)
    const legacy = fake.lastSocket()
    legacy.open('legacy')
    expect(handle.reason?.code).toBe('OBSERVER_ID_CONFLICT')
    expect(() => sdk.observe({ function_id: 'orders::create' }, 'audit::record', { id: 'r' })).toThrow(/already in use/)
    expect(await timedDispose(handle)).toBeLessThan(DISPOSE_WAIT_MS / 10)
    expect(observeFrames(next)).toEqual([])
    expect(observeFrames(legacy)).toEqual([])
    expect(states).toEqual(['error', 'disposed'])
  })

  it('an error state change keeps an active handle held: it is replayed, and its dispose() sends unobserve', async () => {
    vi.useFakeTimers()
    const { sdk, socket } = worker()
    socket.open()
    const handle = sdk.observe({ function_id: 'orders::create' }, 'audit::record', { id: 'r' })
    socket.deliver({ type: 'observeresult', id: 'r', state: 'active' })
    // Its host rejected it: the engine holds the observer in error.
    socket.deliver({
      type: 'observerstatechanged',
      id: 'r',
      state: 'error',
      fires: 0,
      reason: { code: 'REJECTED', message: 'rejected by its host' },
    })
    expect(handle.state).toBe('error')
    socket.drop()
    await vi.advanceTimersByTimeAsync(50)
    const next = fake.lastSocket()
    next.open()
    expect(observeFrames(next)).toEqual(['observe:r'])
    next.deliver({ type: 'observeresult', id: 'r', state: 'active' })
    expect(handle.state).toBe('active')
    next.deliver({
      type: 'observerstatechanged',
      id: 'r',
      state: 'error',
      fires: 0,
      reason: { code: 'REJECTED', message: 'rejected by its host' },
    })
    await handle.dispose()
    expect(observeFrames(next)).toEqual(['observe:r', 'unobserve:r'])
  })

  it('an error answer to the replay of an observer held in error is a refusal: dispose() sends nothing and the id is free', async () => {
    vi.useFakeTimers()
    const { sdk, socket } = worker()
    socket.open()
    const handle = sdk.observe({ function_id: 'orders::create' }, 'audit::record', { id: 'r' })
    socket.deliver({ type: 'observeresult', id: 'r', state: 'active' })
    socket.deliver({
      type: 'observerstatechanged',
      id: 'r',
      state: 'error',
      fires: 0,
      reason: { code: 'REJECTED', message: 'rejected by its host' },
    })
    socket.drop()
    await vi.advanceTimersByTimeAsync(50)
    const next = fake.lastSocket()
    next.open()
    // The engine refuses the replay with the observer's error and disposes it as it answers.
    next.deliver({
      type: 'observeresult',
      id: 'r',
      state: 'error',
      error: { code: 'REJECTED', message: 'rejected by its host' },
    })
    expect(handle.state).toBe('error')
    expect(handle.reason?.code).toBe('REJECTED')
    // Out of the id's list, and not sent again on a later connection.
    next.deliver({ type: 'observerstatechanged', id: 'r', state: 'active', fires: 0 })
    expect(handle.state).toBe('error')
    next.drop()
    await vi.advanceTimersByTimeAsync(50)
    const after = fake.lastSocket()
    after.open()
    expect(observeFrames(after)).toEqual([])
    expect(await timedDispose(handle)).toBeLessThan(DISPOSE_WAIT_MS / 10)
    expect(observeFrames(next)).toEqual(['observe:r'])
    expect(observeFrames(after)).toEqual([])
    // The id observed again is a new observer, which hears its own answer.
    const again = sdk.observe({ function_id: 'orders::create' }, 'audit::record', { id: 'r' })
    after.deliver({ type: 'observeresult', id: 'r', state: 'active' })
    expect(again.state).toBe('active')
    await again.dispose()
    expect(observeFrames(after)).toEqual(['observe:r', 'unobserve:r'])
  })

  it('an error answer to the replay of an observe whose answer a dropped connection lost is a refusal: dispose() sends nothing', async () => {
    vi.useFakeTimers()
    const { sdk, socket } = worker()
    socket.open()
    const handle = sdk.observe({ function_id: 'orders::create' }, 'audit::record', { id: 'r' })
    socket.drop()
    await vi.advanceTimersByTimeAsync(50)
    const next = fake.lastSocket()
    next.open()
    next.deliver({
      type: 'observeresult',
      id: 'r',
      state: 'error',
      error: { code: 'REJECTED', message: 'rejected by its host' },
    })
    expect(handle.state).toBe('error')
    expect(await timedDispose(handle)).toBeLessThan(DISPOSE_WAIT_MS / 10)
    expect(handle.state).toBe('disposed')
    expect(observeFrames(next)).toEqual(['observe:r'])
  })
})
