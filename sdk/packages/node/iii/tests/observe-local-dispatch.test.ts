import { afterEach, describe, expect, it, vi } from 'vitest'
import type { InvocationContext } from '../src/observe-types'
import type { IIIClient } from '../src/types'

/** Local dispatch, the invocation context and caller propagation against a fake engine (sdk.md §3, §4.3). */
vi.mock('ws', async () => (await import('./observe-fake-engine')).wsModule())
vi.resetModules()
const fake = await import('./observe-fake-engine')
const { registerWorker, TriggerAction } = await import('../src/index')
const { InvocationError } = await import('../src/errors')
const { functionRegistered, observerInfo, settle, reportedItems } = fake

let sdk: IIIClient | undefined
afterEach(async () => {
  vi.restoreAllMocks()
  vi.useRealTimers()
  await sdk?.shutdown()
  sdk = undefined
})

function worker(opts: { namespace?: string; reporter?: object } = {}) {
  sdk = registerWorker('ws://local.test', {
    otel: { enabled: false },
    enableMetricsReporting: false,
    workerName: 'billing',
    namespace: opts.namespace,
    reconnectionConfig: { initialDelayMs: 10, jitterFactor: 0 },
    observe: { reporter: opts.reporter },
  } as never)
  return { sdk, socket: fake.lastSocket() }
}

const invokes = (socket: ReturnType<typeof fake.lastSocket>) =>
  socket.framesOf('invokefunction').filter((f) => f.function_id !== 'engine::workers::register')

describe('the local path: when trigger() runs in-process (sdk.md §3)', () => {
  it('runs a confirmed function in-process and puts nothing on the wire', async () => {
    const { sdk, socket } = worker()
    const handler = vi.fn(async (input: { n: number }) => ({ doubled: input.n * 2 }))
    sdk.registerFunction('math::double', handler)
    socket.open()
    socket.deliver(functionRegistered('math::double'))
    socket.clear()
    await expect(sdk.trigger({ function_id: 'math::double', payload: { n: 21 } })).resolves.toEqual({ doubled: 42 })
    expect(handler).toHaveBeenCalledTimes(1)
    expect(socket.sent).toHaveLength(0)
  })

  it.each([
    ['not confirmed yet', { confirm: false }],
    ['local_dispatch: false from the engine', { localDispatch: false }],
    ['the localDispatch: false option', { option: false }],
    ['another namespace', { namespace: 'other' }],
    ["action 'enqueue'", { enqueue: true }],
    ["dispatch: 'remote'", { remote: true }],
  ] as const)('goes to the engine when %s', async (_name, c: Record<string, unknown>) => {
    const { sdk, socket } = worker()
    const handler = vi.fn(async () => 'local')
    sdk.registerFunction('math::double', handler, c.option === false ? { localDispatch: false } : undefined)
    socket.open()
    if (c.confirm !== false) {
      socket.deliver(functionRegistered('math::double', [], { local_dispatch: c.localDispatch !== false }))
    }
    socket.clear()
    void sdk
      .trigger({
        function_id: 'math::double',
        payload: {},
        timeoutMs: 20,
        namespace: c.namespace as string | undefined,
        action: c.enqueue ? TriggerAction.Enqueue({ queue: 'q' }) : undefined,
        dispatch: c.remote ? 'remote' : undefined,
      })
      .catch(() => undefined)
    expect(invokes(socket)).toHaveLength(1)
    expect(invokes(socket)[0]).not.toHaveProperty('dispatch')
    expect(handler).not.toHaveBeenCalled()
  })

  it('stays local for the namespace the function registered in, named or not', async () => {
    const { sdk, socket } = worker({ namespace: 'orders' })
    const handler = vi.fn(async () => 'local')
    sdk.registerFunction('orders::archive', handler)
    socket.open()
    socket.deliver(functionRegistered('orders::archive', [], { namespace: 'orders' }))
    socket.clear()
    await sdk.trigger({ function_id: 'orders::archive', payload: {} })
    await sdk.trigger({ function_id: 'orders::archive', payload: {}, namespace: 'orders' })
    expect(handler).toHaveBeenCalledTimes(2)
    expect(socket.sent).toHaveLength(0)
  })

  it('a local call nested more than 64 local calls deep goes to the engine', async () => {
    const { sdk, socket } = worker()
    let local = 0
    sdk.registerFunction('rec::down', async (n: number): Promise<number> => {
      local++
      return n === 0 ? 0 : sdk.trigger({ function_id: 'rec::down', payload: n - 1 })
    })
    socket.open()
    socket.deliver(functionRegistered('rec::down'))
    socket.clear()
    const result = sdk.trigger({ function_id: 'rec::down', payload: 100 })
    await settle()
    expect(local).toBe(64)
    const [remote] = invokes(socket)
    expect(remote).toMatchObject({ function_id: 'rec::down', data: 36 })
    socket.deliver({
      type: 'invocationresult',
      invocation_id: remote.invocation_id,
      function_id: 'rec::down',
      result: 0,
    })
    await expect(result).resolves.toBe(0)
  })

  it('never runs an HTTP-invoked function locally', async () => {
    const { sdk, socket } = worker()
    sdk.registerFunction('ext::lambda', { url: 'https://example.invalid' })
    socket.open()
    socket.deliver(functionRegistered('ext::lambda'))
    socket.clear()
    void sdk.trigger({ function_id: 'ext::lambda', payload: {}, timeoutMs: 20 }).catch(() => undefined)
    expect(invokes(socket)).toHaveLength(1)
  })

  it('void runs locally, fire-and-forget', async () => {
    const { sdk, socket } = worker()
    let release!: () => void
    const ran = vi.fn()
    sdk.registerFunction('jobs::run', async () => {
      await new Promise<void>((r) => {
        release = r
      })
      ran()
    })
    socket.open()
    socket.deliver(functionRegistered('jobs::run'))
    socket.clear()
    await expect(
      sdk.trigger({ function_id: 'jobs::run', payload: {}, action: TriggerAction.Void() }),
    ).resolves.toBeUndefined()
    expect(ran).not.toHaveBeenCalled()
    release()
    await settle()
    expect(ran).toHaveBeenCalledTimes(1)
    expect(socket.sent).toHaveLength(0)
  })

  it('keeps serving locally while disconnected (local dispatch survives a disconnect)', async () => {
    vi.useFakeTimers()
    const { sdk, socket } = worker()
    sdk.registerFunction('math::double', async (n: number) => n * 2)
    socket.open()
    socket.deliver(functionRegistered('math::double'))
    socket.drop()
    await expect(sdk.trigger({ function_id: 'math::double', payload: 4 })).resolves.toBe(8)
  })
})

describe('value semantics, errors and timeouts: the same as the remote path', () => {
  it('a handler that mutates its input does not affect the caller, and the reverse', async () => {
    const { sdk, socket } = worker()
    const kept: { list: number[] } = { list: [1] }
    sdk.registerFunction('val::mutate', async (input: { list: number[]; at: Date }) => {
      input.list.push(99)
      expect(input.at).toBeInstanceOf(Date)
      return kept
    })
    socket.open()
    socket.deliver(functionRegistered('val::mutate'))
    const payload = { list: [1, 2], at: new Date(0) }
    const result = await sdk.trigger<typeof payload, { list: number[] }>({ function_id: 'val::mutate', payload })
    expect(payload.list).toEqual([1, 2])
    expect(result).toEqual({ list: [1] })
    expect(result).not.toBe(kept)
    result.list.push(7)
    expect(kept.list).toEqual([1])
  })

  it('falls back to JSON for values structuredClone cannot copy', async () => {
    const { sdk, socket } = worker()
    sdk.registerFunction('val::echo', async (input: unknown) => input)
    socket.open()
    socket.deliver(functionRegistered('val::echo'))
    await expect(sdk.trigger({ function_id: 'val::echo', payload: { a: 1, f: () => 1 } })).resolves.toEqual({ a: 1 })
  })

  it('a failing handler rejects with the InvocationError the remote path gives', async () => {
    const { sdk, socket } = worker()
    sdk.registerFunction('err::local', async () => {
      throw new Error('boom')
    })
    socket.open()
    socket.deliver(functionRegistered('err::local'))
    const local = await sdk.trigger({ function_id: 'err::local', payload: {} }).catch((e) => e)

    // the remote path: the engine forwards the handler's error body
    const remote = sdk.trigger({ function_id: 'err::remote', payload: {} }).catch((e) => e)
    const frame = invokes(socket).find((f) => f.function_id === 'err::remote') as Record<string, unknown>
    socket.deliver({
      type: 'invocationresult',
      invocation_id: frame.invocation_id,
      function_id: 'err::remote',
      error: { code: 'invocation_failed', message: 'boom', stacktrace: 'Error: boom\n    at x' },
    })
    const remoteError = await remote
    for (const e of [local, remoteError]) {
      expect(e).toBeInstanceOf(InvocationError)
      expect(e.code).toBe('invocation_failed')
      expect(e.message).toBe('invocation_failed: boom')
      expect(typeof e.stacktrace).toBe('string')
    }
    expect(local.function_id).toBe('err::local')
  })

  it('honours the caller timeout while the handler is still running, and survives its late result', async () => {
    fake.fakeClock()
    const { sdk, socket } = worker()
    let release!: (v: number) => void
    sdk.registerFunction(
      'slow::late',
      () =>
        new Promise<number>((r) => {
          release = r
        }),
    )
    socket.open()
    socket.deliver(functionRegistered('slow::late'))
    await settle()
    const call = sdk.trigger({ function_id: 'slow::late', payload: {}, timeoutMs: 50 })
    const outcome = call.catch((e) => e)
    await fake.elapse(50)
    expect(await outcome).toMatchObject({ code: 'TIMEOUT', function_id: 'slow::late' })
    release(1) // the handler finishes after the caller gave up: nothing breaks
    await settle()
  })

  it('times out like the remote path', async () => {
    const { sdk, socket } = worker()
    sdk.registerFunction('slow::fn', () => new Promise(() => undefined))
    socket.open()
    socket.deliver(functionRegistered('slow::fn'))
    await expect(sdk.trigger({ function_id: 'slow::fn', payload: {}, timeoutMs: 30 })).rejects.toMatchObject({
      code: 'TIMEOUT',
      message: 'TIMEOUT: invocation timed out after 30ms',
      function_id: 'slow::fn',
    })
  })
})

describe('the invocation context and caller propagation', () => {
  it('currentInvocation() in a local handler, and the caller chain of nested local calls', async () => {
    const { sdk, socket } = worker()
    const seen: Record<string, InvocationContext | undefined> = {}
    sdk.registerFunction('chain::outer', async () => {
      seen.outer = sdk.currentInvocation()
      return sdk.trigger({ function_id: 'chain::inner', payload: {} })
    })
    sdk.registerFunction('chain::inner', async () => {
      seen.inner = sdk.currentInvocation()
      return 'done'
    })
    socket.open()
    socket.deliver(functionRegistered('chain::outer'))
    socket.deliver(functionRegistered('chain::inner'))
    expect(sdk.currentInvocation()).toBeUndefined()
    await sdk.trigger({ function_id: 'chain::outer', payload: {} })
    expect(seen.outer).toEqual({
      function_id: 'chain::outer',
      invocation_id: expect.any(String),
      caller: { worker_id: 'worker-1', worker_name: 'billing' },
      observe_depth: 0,
    })
    expect(seen.inner).toEqual({
      function_id: 'chain::inner',
      invocation_id: expect.any(String),
      caller: {
        worker_id: 'worker-1',
        worker_name: 'billing',
        function_id: 'chain::outer',
        invocation_id: seen.outer?.invocation_id,
        parent: { worker_id: 'worker-1', worker_name: 'billing' },
      },
      observe_depth: 0,
    })
  })

  it('an inbound activation sets the context, is answered, and calls made from it carry caller and observe_depth', async () => {
    const { sdk, socket } = worker()
    let seen: InvocationContext | undefined
    sdk.registerFunction('alerts::notify', async () => {
      seen = sdk.currentInvocation()
      void sdk.trigger({ function_id: 'mail::send', payload: {}, timeoutMs: 20 }).catch(() => undefined)
      return null
    })
    socket.open()
    socket.clear()
    socket.deliver({
      type: 'invokefunction',
      invocation_id: 'act-inv-1',
      function_id: 'alerts::notify',
      data: {},
      caller: { worker_name: 'iii-engine', function_id: 'payments::charge', invocation_id: 'inv-1' },
      observe_depth: 2,
      activation: { observer_id: 'obs-1', activation_id: 'act-1' },
    })
    await settle()
    expect(seen).toEqual({
      function_id: 'alerts::notify',
      invocation_id: 'act-inv-1',
      caller: { worker_name: 'iii-engine', function_id: 'payments::charge', invocation_id: 'inv-1' },
      activation: { observer_id: 'obs-1', activation_id: 'act-1' },
      observe_depth: 2,
    })
    const out = invokes(socket).find((f) => f.function_id === 'mail::send')
    expect(out?.caller).toEqual({
      function_id: 'alerts::notify',
      invocation_id: seen?.invocation_id,
      parent: { worker_name: 'iii-engine', function_id: 'payments::charge', invocation_id: 'inv-1' },
    })
    expect(out?.observe_depth).toBe(2)
    // an activation is answered like any call: the engine frees its in-flight slot on the reply (CONTRACT D16)
    expect(socket.framesOf('invocationresult')).toMatchObject([{ invocation_id: 'act-inv-1', result: null }])
  })

  it('omits observe_depth when 0, and caller outside any handler', async () => {
    const { sdk, socket } = worker()
    sdk.registerFunction('a::b', async () => {
      void sdk.trigger({ function_id: 'x::inner', payload: {}, timeoutMs: 20 }).catch(() => undefined)
      return null
    })
    socket.open()
    socket.deliver({ type: 'invokefunction', invocation_id: 'i-1', function_id: 'a::b', data: {} })
    await settle()
    void sdk.trigger({ function_id: 'x::outer', payload: {}, timeoutMs: 20 }).catch(() => undefined)
    const inner = invokes(socket).find((f) => f.function_id === 'x::inner')
    const outer = invokes(socket).find((f) => f.function_id === 'x::outer')
    expect(inner?.caller).toEqual({ function_id: 'a::b', invocation_id: 'i-1' })
    expect(inner).not.toHaveProperty('observe_depth')
    expect(outer).not.toHaveProperty('caller')
  })

  it('sends no caller to an engine without observe.v1', async () => {
    const { sdk, socket } = worker()
    sdk.registerFunction('a::b', async () => {
      void sdk.trigger({ function_id: 'x::inner', payload: {}, timeoutMs: 20 }).catch(() => undefined)
      return null
    })
    socket.open('legacy')
    socket.deliver({ type: 'invokefunction', invocation_id: 'i-1', function_id: 'a::b', data: {} })
    await settle()
    const inner = invokes(socket).find((f) => f.function_id === 'x::inner')
    expect(inner).toBeDefined()
    expect(inner).not.toHaveProperty('caller')
  })
})

describe('auto delivery: local invocations are evaluated and reported (sdk.md §4.3)', () => {
  async function observed(observers: Record<string, unknown>[], reporter?: object) {
    fake.fakeClock()
    const { sdk, socket } = worker({ reporter })
    sdk.registerFunction('payments::charge', async (input: { amount: number; fail?: boolean }) => {
      if (input.fail) throw new Error('card declined')
      ;(input as Record<string, unknown>).mutated = true
      return { charged: input.amount }
    })
    socket.open()
    socket.deliver(functionRegistered('payments::charge', observers))
    await settle()
    socket.clear()
    return { sdk, socket }
  }

  it('reports a matched call once, with the union of the matched observers captures', async () => {
    const { sdk, socket } = await observed([
      observerInfo('big', 'alerts::big', {
        filters: { $and: [{ 'input.amount': { $gte: 500 } }] },
        capture: ['input'],
      }),
      observerInfo('out', 'audit::out', { filters: { $and: [{ 'output.charged': { $gt: 0 } }] }, capture: ['output'] }),
      observerInfo('errors', 'alerts::err', { on: 'error' }),
    ])
    await sdk.trigger({ function_id: 'payments::charge', payload: { amount: 750 }, metadata: { tenant: 'acme' } })
    await sdk.trigger({ function_id: 'payments::charge', payload: { amount: 0 } })
    await fake.elapse(10)
    const items = reportedItems(socket)
    expect(items).toHaveLength(1)
    expect(items[0]).toEqual({
      function_id: 'payments::charge',
      namespace: 'default',
      invocation_id: expect.any(String),
      matched: ['big', 'out'],
      status: 'success',
      input: { amount: 750 }, // as the caller passed it, not as the handler left it
      output: { charged: 750 },
      started_at_ms: expect.any(Number),
      duration_ms: expect.any(Number),
      observe_depth: 0,
    })
  })

  it("on: 'error' sees only failures, with the error and the caller", async () => {
    const { sdk, socket } = await observed([
      observerInfo('errors', 'alerts::err', { on: 'error', capture: ['input', 'error', 'caller'] }),
      observerInfo('all', 'audit::all', { on: 'complete', capture: [] }),
    ])
    await sdk.trigger({ function_id: 'payments::charge', payload: { amount: 1, fail: true } }).catch(() => undefined)
    await sdk.trigger({ function_id: 'payments::charge', payload: { amount: 2 } })
    await fake.elapse(10)
    const items = reportedItems(socket)
    expect(items.map((i) => i.matched)).toEqual([['errors', 'all'], ['all']])
    expect(items[0]).toMatchObject({
      status: 'error',
      input: { amount: 1, fail: true },
      error: { code: 'invocation_failed', message: 'card declined' },
      caller: { worker_id: 'worker-1', worker_name: 'billing' },
    })
    expect(items[1]).not.toHaveProperty('input')
    expect(items[1]).not.toHaveProperty('output')
  })

  it('reports nothing for a call no filter matches, and nothing at all for an unobserved function', async () => {
    const { sdk, socket } = await observed([
      observerInfo('big', 'alerts::big', { filters: { $and: [{ 'input.amount': { $gte: 500 } }] } }),
    ])
    for (let i = 0; i < 100; i++) await sdk.trigger({ function_id: 'payments::charge', payload: { amount: i } })
    await fake.elapse(10)
    expect(socket.sent).toHaveLength(0)
  })

  it('reports the input and metadata as they were when the call started', async () => {
    fake.fakeClock()
    const { sdk, socket } = worker()
    let release!: () => void
    sdk.registerFunction('snap::fn', async (input: { n: number }) => {
      await new Promise<void>((r) => {
        release = r
      })
      return input.n
    })
    socket.open()
    socket.deliver(
      functionRegistered('snap::fn', [observerInfo('o', 'audit::snap', { capture: ['input', 'metadata'] })]),
    )
    await settle()
    socket.clear()
    const payload = { n: 1 }
    const metadata = { tenant: 'acme' }
    const call = sdk.trigger({ function_id: 'snap::fn', payload, metadata })
    await settle()
    payload.n = 99 // the caller changes its objects while the call runs
    metadata.tenant = 'other'
    release()
    await call
    await fake.elapse(10)
    expect(reportedItems(socket).map((i) => [i.input, i.metadata])).toEqual([[{ n: 1 }, { tenant: 'acme' }]])
  })

  it('evaluates a call against the observers active when it started', async () => {
    fake.fakeClock()
    const { sdk, socket } = worker()
    let release!: () => void
    sdk.registerFunction('slow::op', async (n: number) => {
      if (n === 1) {
        await new Promise<void>((r) => {
          release = r
        })
      }
      return n
    })
    socket.open()
    socket.deliver(functionRegistered('slow::op', [], { revision: 1 }))
    await settle()
    socket.clear()
    const first = sdk.trigger({ function_id: 'slow::op', payload: 1 }) // starts unobserved
    await settle()
    socket.deliver(fake.observerAdded('slow::op', 2, observerInfo('late', 'audit::late', { capture: ['input'] })))
    await settle()
    release()
    await first
    await sdk.trigger({ function_id: 'slow::op', payload: 2 })
    await fake.elapse(10)
    expect(reportedItems(socket).map((i) => i.input)).toEqual([2])
  })

  it(
    'zero-wire: 1 000 000 local calls of an unobserved function send no per-call frame',
    { timeout: 120_000 },
    async () => {
      // On a fake clock, so the 10 s localinvocationstats timer cannot fire
      // however long the loop takes; that frame counts calls, it is not per call.
      fake.fakeClock()
      const { sdk, socket } = worker()
      sdk.registerFunction('hot::path', async (n: number) => n + 1)
      socket.open()
      socket.deliver(functionRegistered('hot::path'))
      await settle()
      socket.clear()
      for (let i = 0; i < 1_000_000; i++) await sdk.trigger({ function_id: 'hot::path', payload: i })
      await fake.elapse(10)
      expect(socket.frames().filter((f) => f.type !== 'localinvocationstats')).toEqual([])
      await fake.elapse(10_000)
      expect(socket.framesOf('localinvocationstats')).toMatchObject([
        { functions: [{ function_id: 'hot::path', count: 1_000_000, errors: 0 }] },
      ])
      // Positive control, same harness and counter: once observed, the same calls reach the wire.
      socket.deliver(fake.observerAdded('hot::path', 1, observerInfo('watch', 'audit::hot', { capture: ['input'] })))
      await settle()
      socket.clear()
      for (let i = 0; i < 10; i++) await sdk.trigger({ function_id: 'hot::path', payload: i })
      await fake.elapse(10)
      expect(socket.framesOf('invocationsobserved').length).toBeGreaterThan(0)
      expect(reportedItems(socket)).toHaveLength(10)
    },
  )

  it('counts local invocations in localinvocationstats, only when non-zero', async () => {
    fake.fakeClock()
    const { sdk, socket } = worker()
    sdk.registerFunction('math::inc', async (n: number) => {
      if (n < 0) throw new Error('negative')
      return n + 1
    })
    socket.open()
    socket.deliver(functionRegistered('math::inc'))
    socket.clear()
    await fake.elapse(20_000)
    expect(socket.framesOf('localinvocationstats')).toHaveLength(0)
    await sdk.trigger({ function_id: 'math::inc', payload: 1 })
    await sdk.trigger({ function_id: 'math::inc', payload: -1 }).catch(() => undefined)
    await fake.elapse(10_000)
    const stats = socket.framesOf('localinvocationstats')
    expect(stats).toHaveLength(1)
    expect(stats[0]).toMatchObject({
      functions: [{ function_id: 'math::inc', count: 2, errors: 1, duration_ms_sum: expect.any(Number) }],
      dropped_reports: [],
    })
  })
})

describe('the reporter against the socket and the function registry', () => {
  const statsDrops = (socket: ReturnType<typeof fake.lastSocket>) =>
    socket.framesOf('localinvocationstats').flatMap((f) => f.dropped_reports as Record<string, unknown>[])

  it('a peer that stops reading cannot grow memory: unwritten bytes count against the bound', async () => {
    fake.fakeClock()
    const { sdk, socket } = worker({ reporter: { maxBytes: 64 * 1024 } })
    sdk.registerFunction('big::fn', async (input: { blob: string }) => input.blob.length)
    socket.open()
    socket.deliver(functionRegistered('big::fn', [observerInfo('o', 'audit::big', { capture: ['input'] })]))
    await settle()
    // ws keeps what it cannot write: bufferedAmount only grows
    const send = socket.send.bind(socket)
    socket.send = (data, callback) => {
      socket.bufferedAmount += Buffer.byteLength(String(data))
      send(data, callback)
    }
    socket.clear()
    const blob = 'z'.repeat(2048)
    for (let i = 0; i < 500; i++) {
      await sdk.trigger({ function_id: 'big::fn', payload: { blob, i } })
      if (i % 50 === 0) await fake.elapse(10)
    }
    await fake.elapse(500)
    const handedOver = socket.framesOf('invocationsobserved').reduce((n, f) => n + JSON.stringify(f).length, 0)
    expect(handedOver).toBeGreaterThan(0)
    expect(handedOver).toBeLessThan(64 * 1024 + 70 * 1024) // the bound plus at most one frame in flight
    await fake.elapse(10_000)
    expect(statsDrops(socket)).toEqual([expect.objectContaining({ function_id: 'big::fn', reason: 'BUFFER_FULL' })])
  })

  it('FUNCTION_NAMESPACE_CONFLICT and unregister() discard the function’s held reports as INACTIVE; others still go out', async () => {
    fake.fakeClock()
    const { sdk, socket } = worker()
    const fns: Record<string, ReturnType<IIIClient['registerFunction']>> = {}
    for (const id of ['f::one', 'f::two', 'f::three']) {
      fns[id] = sdk.registerFunction(id, async (n: number) => n)
    }
    socket.open()
    for (const id of Object.keys(fns)) {
      socket.deliver(functionRegistered(id, [observerInfo(`o-${id}`, 'audit::any', { capture: ['input'] })]))
    }
    await settle()
    socket.drop() // reports are held while disconnected
    for (const id of Object.keys(fns)) {
      for (let i = 0; i < 3; i++) await sdk.trigger({ function_id: id, payload: i })
    }
    const reporter = (sdk as unknown as { reporter: { size: number; drops: Map<string, Map<string, number>> } })
      .reporter
    expect(reporter.size).toBe(9)
    fns['f::three'].unregister() // discarded now, not at the next flush
    expect(reporter.size).toBe(6)
    expect(reporter.drops.get('f::three')?.get('INACTIVE')).toBe(3)
    await fake.elapse(1500)
    const next = fake.lastSocket()
    next.open()
    next.deliver({
      type: 'registrationrejected',
      code: 'FUNCTION_NAMESPACE_CONFLICT',
      namespace: 'default',
      function_id: 'f::one',
      owner_worker_id: 'other',
    })
    next.deliver(functionRegistered('f::two', [observerInfo('o-f::two', 'audit::any', { capture: ['input'] })]))
    await settle()
    await fake.elapse(10)
    expect(reportedItems(next).map((i) => [i.function_id, i.input])).toEqual([
      ['f::two', 0],
      ['f::two', 1],
      ['f::two', 2],
    ])
    await fake.elapse(10_000)
    expect(statsDrops(next)).toEqual([
      { function_id: 'f::three', reason: 'INACTIVE', count: 3 },
      { function_id: 'f::one', reason: 'INACTIVE', count: 3 },
    ])
  })

  it('a local call already running when the function is refused never leaves a held report', async () => {
    fake.fakeClock()
    const { sdk, socket } = worker()
    let release!: () => void
    sdk.registerFunction('f::slow', async (n: number) => {
      await new Promise<void>((r) => {
        release = r
      })
      return n
    })
    socket.open()
    socket.deliver(functionRegistered('f::slow', [observerInfo('o', 'audit::any', { capture: ['input'] })]))
    await settle()
    const call = sdk.trigger({ function_id: 'f::slow', payload: 1 }) // evaluated against the set at start
    await settle()
    socket.deliver({
      type: 'registrationrejected',
      code: 'FUNCTION_NAMESPACE_CONFLICT',
      namespace: 'default',
      function_id: 'f::slow',
      owner_worker_id: 'other',
    })
    await settle()
    release()
    await expect(call).resolves.toBe(1)
    const reporter = (sdk as unknown as { reporter: { size: number; drops: Map<string, Map<string, number>> } })
      .reporter
    await fake.elapse(10) // the flush: still refused, so gone
    expect(reporter.size).toBe(0)
    expect(reporter.drops.get('f::slow')?.get('INACTIVE')).toBe(1)
    expect(reportedItems(socket)).toEqual([])
  })

  const conflict = (fn: string) => ({
    type: 'registrationrejected',
    code: 'FUNCTION_NAMESPACE_CONFLICT',
    namespace: 'default',
    function_id: fn,
    owner_worker_id: 'other',
  })

  it('a refused function confirmed again on a reconnect reports its later local calls', async () => {
    fake.fakeClock()
    const { sdk, socket } = worker()
    sdk.registerFunction('f::back', async (n: number) => n)
    socket.open()
    socket.deliver(functionRegistered('f::back', [observerInfo('o', 'audit::any', { capture: ['input'] })]))
    await settle()
    socket.deliver(conflict('f::back'))
    await settle()
    socket.drop()
    await fake.elapse(1500)
    const next = fake.lastSocket()
    next.open() // the other owner left: the replayed registration is accepted
    next.deliver(functionRegistered('f::back', [observerInfo('o', 'audit::any', { capture: ['input'] })]))
    await settle()
    await sdk.trigger({ function_id: 'f::back', payload: 7 })
    await fake.elapse(10)
    expect(reportedItems(next).map((i) => i.input)).toEqual([7])
  })

  it('a function refused and then confirmed on the same connection reports again (functionregistered clears the refusal)', async () => {
    fake.fakeClock()
    const { sdk, socket } = worker()
    sdk.registerFunction('f::same', async (n: number) => n)
    socket.open()
    socket.deliver(functionRegistered('f::same', [observerInfo('o', 'audit::any', { capture: ['input'] })]))
    await settle()
    socket.deliver(conflict('f::same'))
    await settle()
    socket.deliver(functionRegistered('f::same', [observerInfo('o', 'audit::any', { capture: ['input'] })]))
    await settle()
    await sdk.trigger({ function_id: 'f::same', payload: 3 })
    await fake.elapse(10)
    expect(reportedItems(socket).map((i) => i.input)).toEqual([3])
  })

  it('a call running across conflict → disconnect → re-confirmation is reported, not dropped', async () => {
    fake.fakeClock()
    const { sdk, socket } = worker()
    let release!: () => void
    sdk.registerFunction('f::span', async (n: number) => {
      await new Promise<void>((r) => {
        release = r
      })
      return n
    })
    socket.open()
    socket.deliver(functionRegistered('f::span', [observerInfo('o', 'audit::any', { capture: ['input'] })]))
    await settle()
    const call = sdk.trigger({ function_id: 'f::span', payload: 1 })
    await settle()
    socket.deliver(conflict('f::span'))
    await settle()
    socket.drop()
    release()
    await expect(call).resolves.toBe(1) // completes while disconnected: held, not dropped
    await fake.elapse(1500)
    const next = fake.lastSocket()
    next.open()
    next.deliver(functionRegistered('f::span', [observerInfo('o', 'audit::any', { capture: ['input'] })]))
    await settle()
    await fake.elapse(10)
    expect(reportedItems(next).map((i) => [i.input, i.matched])).toEqual([[1, ['o']]])
  })

  it('a call running across unregister + register of the same id is reported for the new registration', async () => {
    fake.fakeClock()
    const { sdk, socket } = worker()
    let release!: () => void
    const handler = async (n: number) => {
      await new Promise<void>((r) => {
        release = r
      })
      return n
    }
    const first = sdk.registerFunction('f::again', handler)
    socket.open()
    socket.deliver(functionRegistered('f::again', [observerInfo('o', 'audit::any', { capture: ['input'] })]))
    await settle()
    const call = sdk.trigger({ function_id: 'f::again', payload: 2 })
    await settle()
    first.unregister()
    sdk.registerFunction('f::again', async (n: number) => n)
    socket.deliver(functionRegistered('f::again', [observerInfo('o', 'audit::any', { capture: ['input'] })]))
    await settle()
    release()
    await expect(call).resolves.toBe(2)
    await fake.elapse(10)
    expect(reportedItems(socket).map((i) => [i.input, i.matched])).toEqual([[2, ['o']]])
  })

  it('held reports of a function are neither expired nor discarded while its hooks run after a reconnect', async () => {
    fake.fakeClock()
    const { sdk, socket } = worker()
    let release!: () => void
    sdk.registerFunction('f::hooked', async (n: number) => n, {
      observe: {
        onObserverAdded: async (obs) => {
          if (obs.id === 'slow') {
            await new Promise<void>((r) => {
              release = r
            })
          }
        },
      },
    })
    socket.open()
    socket.deliver(functionRegistered('f::hooked', [observerInfo('a', 'audit::a', { capture: ['input'] })]))
    await settle()
    socket.drop()
    for (let i = 0; i < 3; i++) await sdk.trigger({ function_id: 'f::hooked', payload: i }) // held: disconnected
    await fake.elapse(1500)
    const next = fake.lastSocket()
    next.open()
    next.deliver(
      functionRegistered('f::hooked', [
        observerInfo('a', 'audit::a', { capture: ['input'] }),
        observerInfo('slow', 'audit::slow', { capture: ['input'] }),
      ]),
    )
    await settle()
    await fake.elapse(25_000) // the slow hook is still running (its timeout is 30 s)
    expect(reportedItems(next)).toEqual([])
    const reporter = (sdk as unknown as { reporter: { size: number } }).reporter
    expect(reporter.size).toBe(3)
    release()
    await settle()
    await fake.elapse(10)
    expect(reportedItems(next).map((i) => [i.input, i.matched])).toEqual([
      [0, ['a']],
      [1, ['a']],
      [2, ['a']],
    ])
  })
})
