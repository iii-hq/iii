import { runInNewContext } from 'node:vm'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { CallAction, registerWorker } from '../src/index'
import { MAX_OFFLINE_LOCAL_DEPTH, MAX_RUNNING_INVOCATION_MS } from '../src/invocation-context'
import type { InvocationContext, Observer } from '../src/observe-types'
import type { ISdk } from '../src/types'
import * as fake from './observe-fake-engine'

/**
 * Browser specifics: the invocation context is a synchronous scope (no
 * `AsyncLocalStorage`), `withInvocation` carries it across an `await`, a
 * deliver hook keeps its invocation for the calls it makes after one, and
 * `call` sends `metadata` / `traceparent` / `baggage`.
 */
const { functionRegistered, observerInfo, settle, reportedItems, invokes } = fake

const TRACEPARENT = '00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01'
const OTHER_TRACEPARENT = '00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01'

let sdk: ISdk | undefined
afterEach(async () => {
  vi.restoreAllMocks()
  await sdk?.shutdown()
  sdk = undefined
  vi.useRealTimers()
})

/** Past the reporter's 5 ms linger (on the fake clock). */
const flushed = () => fake.elapse(10)

/**
 * Runs `fn` and returns the rejections nobody handled meanwhile (vitest's own
 * listeners are set aside while it runs, and put back).
 */
async function unhandledDuring(fn: () => Promise<void>): Promise<unknown[]> {
  const reported: unknown[] = []
  const vitestListeners = process.listeners('unhandledRejection')
  process.removeAllListeners('unhandledRejection')
  const listener = (reason: unknown) => {
    reported.push(reason)
  }
  process.on('unhandledRejection', listener)
  try {
    await fn()
    for (let i = 0; i < 3; i++) await new Promise<void>((r) => setImmediate(r))
  } finally {
    process.off('unhandledRejection', listener)
    for (const l of vitestListeners) process.on('unhandledRejection', l)
  }
  return reported
}

/** An inbound call to `functionId`: an observe activation when `depth` > 0. */
function inbound(functionId: string, invocationId: string, depth: number, extra: Record<string, unknown> = {}) {
  return {
    type: 'invokefunction',
    invocation_id: invocationId,
    function_id: functionId,
    data: {},
    ...(depth > 0
      ? {
          observe_depth: depth,
          caller: { worker_name: 'iii-engine', function_id: 'x::observed', invocation_id: `obs-${invocationId}` },
          activation: { observer_id: 'o-1', activation_id: `act-${invocationId}` },
        }
      : {}),
    ...extra,
  }
}

function worker() {
  fake.installFakeEngine()
  sdk = registerWorker('ws://context.test', { workerName: 'ui' })
  const socket = fake.lastSocket()
  socket.open()
  socket.clear()
  return { sdk, socket }
}

const sent = (socket: fake.FakeSocket, functionId: string) => invokes(socket).find((f) => f.function_id === functionId)

describe('call sends metadata and the trace context', () => {
  it('sends metadata, traceparent and baggage given on the request, on every action', () => {
    const { sdk, socket } = worker()
    const request = {
      payload: { a: 1 },
      metadata: { tenant: 'acme' },
      traceparent: TRACEPARENT,
      baggage: 'k=v',
    }
    void sdk.call({ function_id: 'x::sync', timeoutMs: 20, ...request }).catch(() => undefined)
    void sdk.call({ function_id: 'x::void', action: CallAction.Void(), ...request })
    void sdk
      .call({
        function_id: 'x::queued',
        action: CallAction.Enqueue({ queue: 'q' }),
        timeoutMs: 20,
        ...request,
      })
      .catch(() => undefined)
    for (const id of ['x::sync', 'x::void', 'x::queued']) {
      expect(sent(socket, id)).toMatchObject({
        metadata: { tenant: 'acme' },
        traceparent: TRACEPARENT,
        baggage: 'k=v',
      })
    }
  })

  it('a top-level call without them sends none (the engine starts the trace)', () => {
    const { sdk, socket } = worker()
    void sdk.call({ function_id: 'x::plain', payload: {}, timeoutMs: 20 }).catch(() => undefined)
    const frame = sent(socket, 'x::plain')
    expect(frame).not.toHaveProperty('metadata')
    expect(frame).not.toHaveProperty('traceparent')
    expect(frame).not.toHaveProperty('baggage')
    expect(frame).not.toHaveProperty('caller')
  })

  it('a call made inside a handler continues the trace the handler was invoked with; an explicit one wins', async () => {
    const { sdk, socket } = worker()
    sdk.registerFunction('ui::on-event', async () => {
      void sdk.call({ function_id: 'x::inherits', payload: {}, timeoutMs: 20 }).catch(() => undefined)
      void sdk
        .call({
          function_id: 'x::explicit',
          payload: {},
          timeoutMs: 20,
          traceparent: OTHER_TRACEPARENT,
        })
        .catch(() => undefined)
      return null
    })
    socket.deliver({
      type: 'invokefunction',
      invocation_id: 'inv-1',
      function_id: 'ui::on-event',
      data: {},
      traceparent: TRACEPARENT,
      baggage: 'tenant=acme',
    })
    await settle()
    expect(sent(socket, 'x::inherits')).toMatchObject({
      traceparent: TRACEPARENT,
      baggage: 'tenant=acme',
    })
    expect(sent(socket, 'x::explicit')).toMatchObject({
      traceparent: OTHER_TRACEPARENT,
      baggage: 'tenant=acme',
    })
    // the reply carries the trace context back, as before
    expect(socket.framesOf('invocationresult')[0]).toMatchObject({
      invocation_id: 'inv-1',
      traceparent: TRACEPARENT,
    })
  })

  it("a handler that rethrows a failed call's error body reports that message, on both paths", async () => {
    const { sdk, socket } = worker()
    sdk.registerFunction('ui::inner', async () => {
      throw new Error('inner failed')
    })
    sdk.registerFunction('ui::outer', async () => sdk.call({ function_id: 'ui::inner', payload: {} }))
    // remote: the engine invokes ui::outer; its call to ui::inner is not confirmed for local dispatch
    socket.deliver({ type: 'invokefunction', invocation_id: 'outer-1', function_id: 'ui::outer', data: {} })
    await settle()
    const call = sent(socket, 'ui::inner') as Record<string, unknown>
    socket.deliver({
      type: 'invocationresult',
      invocation_id: call.invocation_id,
      function_id: 'ui::inner',
      error: { code: 'invocation_failed', message: 'inner failed' },
    })
    await settle()
    expect(socket.framesOf('invocationresult')[0]).toMatchObject({
      invocation_id: 'outer-1',
      error: { code: 'invocation_failed', message: 'inner failed' },
    })
    // local: both confirmed, the same body comes back
    socket.deliver(functionRegistered('ui::inner'))
    socket.deliver(functionRegistered('ui::outer'))
    await expect(sdk.call({ function_id: 'ui::outer', payload: {} })).rejects.toMatchObject({
      code: 'invocation_failed',
      message: 'inner failed',
    })
  })

  it('hands the invocation metadata to the handler', async () => {
    const { sdk, socket } = worker()
    const seen: unknown[] = []
    sdk.registerFunction('ui::show', async (_data, metadata) => {
      seen.push(metadata)
      return null
    })
    socket.deliver({
      type: 'invokefunction',
      invocation_id: 'i-1',
      function_id: 'ui::show',
      data: {},
      metadata: { a: 1 },
    })
    socket.deliver({
      type: 'invokefunction',
      invocation_id: 'i-2',
      function_id: 'ui::show',
      data: {},
    })
    socket.deliver(functionRegistered('ui::show'))
    await sdk.call({ function_id: 'ui::show', payload: {}, metadata: { local: true } })
    await settle()
    expect(seen).toEqual([{ a: 1 }, undefined, { local: true }])
  })

  it('a local call takes the trace context into its handler and its report', async () => {
    fake.fakeClock()
    const { sdk, socket } = worker()
    let inner: InvocationContext | undefined
    sdk.registerFunction('ui::traced', async () => {
      inner = sdk.currentInvocation()
      return 'ok'
    })
    socket.deliver(functionRegistered('ui::traced', [observerInfo('o-1', 't::a', { capture: ['input'] })]))
    await settle()
    socket.clear()
    await sdk.call({
      function_id: 'ui::traced',
      payload: { n: 1 },
      traceparent: TRACEPARENT,
      baggage: 'k=v',
    })
    await flushed()
    expect(inner).toMatchObject({
      function_id: 'ui::traced',
      traceparent: TRACEPARENT,
      baggage: 'k=v',
    })
    expect(reportedItems(socket)).toMatchObject([{ input: { n: 1 }, traceparent: TRACEPARENT, baggage: 'k=v' }])
  })
})

describe('the invocation context is a synchronous scope', () => {
  it('is known up to the first await, and not after it', async () => {
    const { sdk, socket } = worker()
    const seen: (InvocationContext | undefined)[] = []
    sdk.registerFunction('ui::two-steps', async () => {
      seen.push(sdk.currentInvocation())
      await Promise.resolve()
      seen.push(sdk.currentInvocation())
      return null
    })
    socket.deliver({
      type: 'invokefunction',
      invocation_id: 'i-1',
      function_id: 'ui::two-steps',
      data: {},
    })
    await settle()
    expect(seen[0]).toMatchObject({ function_id: 'ui::two-steps', invocation_id: 'i-1' })
    expect(seen[1]).toBeUndefined()
    expect(sdk.currentInvocation()).toBeUndefined()
  })

  it("never shows one invocation's context to another, however their awaits interleave", async () => {
    const { sdk, socket } = worker()
    const gates = new Map<string, () => void>()
    const seen: Record<string, (string | undefined)[]> = {}
    sdk.registerFunction('ui::concurrent', async (data: { id: string }) => {
      const mine = [sdk.currentInvocation()?.invocation_id]
      seen[data.id] = mine
      await new Promise<void>((r) => gates.set(data.id, r))
      mine.push(sdk.currentInvocation()?.invocation_id)
      return null
    })
    for (const id of ['a', 'b']) {
      socket.deliver({
        type: 'invokefunction',
        invocation_id: `inv-${id}`,
        function_id: 'ui::concurrent',
        data: { id },
      })
    }
    await settle()
    gates.get('b')?.()
    gates.get('a')?.()
    await settle()
    expect(seen).toEqual({ a: ['inv-a', undefined], b: ['inv-b', undefined] })
  })

  it('is restored when a handler throws synchronously', async () => {
    const { sdk, socket } = worker()
    sdk.registerFunction('ui::throws', (() => {
      throw new Error('sync failure')
    }) as never)
    socket.deliver({
      type: 'invokefunction',
      invocation_id: 'i-1',
      function_id: 'ui::throws',
      data: {},
    })
    await settle()
    expect(sdk.currentInvocation()).toBeUndefined()
    expect(socket.framesOf('invocationresult')[0]).toMatchObject({
      error: { code: 'invocation_failed', message: 'sync failure' },
    })
  })
})

describe('after an await, outside any scope', () => {
  it('one activation running: its calls carry its observe_depth, caller and traceparent', async () => {
    const { sdk, socket } = worker()
    sdk.registerFunction('ui::late-call', async () => {
      await Promise.resolve()
      void sdk.call({ function_id: 'x::after-await', payload: {}, timeoutMs: 20 }).catch(() => undefined)
      return null
    })
    socket.deliver(inbound('ui::late-call', 'i-1', 3, { traceparent: TRACEPARENT }))
    await settle()
    expect(sdk.currentInvocation()).toBeUndefined()
    expect(sent(socket, 'x::after-await')).toMatchObject({
      observe_depth: 3,
      caller: { function_id: 'ui::late-call', invocation_id: 'i-1' },
      traceparent: TRACEPARENT,
    })
  })

  it('several running: the deepest observe_depth (over-counted), and no caller or traceparent', async () => {
    const { sdk, socket } = worker()
    const gates = new Map<string, () => void>()
    sdk.registerFunction('ui::waits', async (data: { id: string }) => {
      await new Promise<void>((r) => gates.set(data.id, r))
      void sdk.call({ function_id: `x::after-${data.id}`, payload: {}, timeoutMs: 20 }).catch(() => undefined)
      return null
    })
    socket.deliver(inbound('ui::waits', 'i-2', 2, { data: { id: 'shallow' }, traceparent: TRACEPARENT }))
    socket.deliver(inbound('ui::waits', 'i-5', 5, { data: { id: 'deep' } }))
    await settle()
    gates.get('shallow')?.()
    await settle()
    const frame = sent(socket, 'x::after-shallow')
    expect(frame).toMatchObject({ observe_depth: 5 })
    expect(frame).not.toHaveProperty('caller')
    expect(frame).not.toHaveProperty('traceparent')
    gates.get('deep')?.()
    await settle()
    // the deep one is the only one running now: unambiguous
    expect(sent(socket, 'x::after-deep')).toMatchObject({ observe_depth: 5, caller: { invocation_id: 'i-5' } })
  })

  it('outside any observe chain the calls stay top-level', async () => {
    const { sdk, socket } = worker()
    sdk.registerFunction('ui::plain', async () => {
      await Promise.resolve()
      void sdk.call({ function_id: 'x::plain-after', payload: {}, timeoutMs: 20 }).catch(() => undefined)
      return null
    })
    socket.deliver(inbound('ui::plain', 'i-1', 0, { traceparent: TRACEPARENT }))
    await settle()
    const frame = sent(socket, 'x::plain-after')
    expect(frame).toBeDefined()
    expect(frame).not.toHaveProperty('caller')
    expect(frame).not.toHaveProperty('observe_depth')
    expect(frame).not.toHaveProperty('traceparent')
  })

  it('the loop guard holds for a target that awaits before it calls back into what it observes', async () => {
    // The test plays the engine: every call to x::loop at depth d activates the
    // observer's target ui::on-loop at depth d + 1, until d + 1 > 8 (max_observe_depth).
    const { sdk, socket } = worker()
    const activations: number[] = []
    sdk.registerFunction('ui::on-loop', async () => {
      activations.push(sdk.currentInvocation()?.observe_depth ?? -1)
      await Promise.resolve() // the scope ends here
      await sdk.call({ function_id: 'x::loop', payload: {} })
      return null
    })
    socket.deliver(inbound('ui::on-loop', 'act-1', 1))
    const depths: number[] = []
    for (let hop = 0; hop < 20; hop++) {
      await settle()
      const calls = invokes(socket).filter((f) => f.function_id === 'x::loop')
      if (calls.length === depths.length) break
      const call = calls[calls.length - 1]
      const depth = (call.observe_depth as number | undefined) ?? 0
      depths.push(depth)
      socket.deliver({ type: 'invocationresult', invocation_id: call.invocation_id, function_id: 'x::loop', result: 0 })
      if (depth + 1 > 8) break
      socket.deliver(inbound('ui::on-loop', `act-${depth + 1}`, depth + 1))
    }
    expect(depths).toEqual([1, 2, 3, 4, 5, 6, 7, 8])
    expect(activations).toEqual([1, 2, 3, 4, 5, 6, 7, 8])
  })

  it('a running call that is not an activation lends its depth, never its caller or trace', async () => {
    const { sdk, socket } = worker()
    sdk.registerFunction('ui::routed', async () => {
      await Promise.resolve()
      void sdk.call({ function_id: 'x::after-routed', payload: {}, timeoutMs: 20 }).catch(() => undefined)
      return null
    })
    // a routed call made from inside some activation elsewhere: deep, but not an activation itself
    socket.deliver(inbound('ui::routed', 'i-1', 0, { observe_depth: 2, traceparent: TRACEPARENT }))
    await settle()
    const frame = sent(socket, 'x::after-routed')
    expect(frame).toMatchObject({ observe_depth: 2 })
    expect(frame).not.toHaveProperty('caller')
    expect(frame).not.toHaveProperty('traceparent')
  })

  it("a call's metadata is its own: a local call after an await carries none of a running activation's", async () => {
    fake.fakeClock()
    const { sdk, socket } = worker()
    sdk.registerFunction('ui::ticks', (tick: { tick: number }) => tick, { private: true })
    let release!: () => void
    sdk.registerFunction('ui::busy', async () => {
      await sdk.call({ function_id: 'ui::ticks', payload: { tick: 0 }, metadata: { page: 'cart' } })
      await new Promise<void>((r) => {
        release = r
      })
      return null
    })
    socket.deliver(
      functionRegistered('ui::ticks', [
        observerInfo('o-1', 't::a', {
          capture: ['input', 'metadata'],
          filters: { $and: [{ 'metadata.page': { $eq: 'cart' } }] },
        }),
      ]),
    )
    await settle()
    socket.clear()
    socket.deliver(inbound('ui::busy', 'busy-1', 2, { metadata: { page: 'cart' } }))
    await settle()
    // a timer, outside any handler, while ui::busy awaits: not ui::busy's metadata
    await sdk.call({ function_id: 'ui::ticks', payload: { tick: 1 } })
    release()
    await settle()
    await flushed()
    expect(reportedItems(socket).map((i) => [i.input, i.metadata])).toEqual([[{ tick: 0 }, { page: 'cart' }]])
  })

  it('a self-recursion that awaits before each call still reaches the engine at 64 local calls', async () => {
    const { sdk, socket } = worker()
    let local = 0
    sdk.registerFunction('rec::await', async (n: number): Promise<number> => {
      local++
      await Promise.resolve() // the scope ends here: the depth comes from the running calls
      return n === 0 ? 0 : sdk.call({ function_id: 'rec::await', payload: n - 1 })
    })
    socket.deliver(functionRegistered('rec::await'))
    await settle()
    socket.clear()
    const result = sdk.call({ function_id: 'rec::await', payload: 100 })
    await settle()
    expect(local).toBe(64)
    const [remote] = invokes(socket)
    expect(remote).toMatchObject({ function_id: 'rec::await', data: 36 })
    socket.deliver({
      type: 'invocationresult',
      invocation_id: remote.invocation_id,
      function_id: 'rec::await',
      result: 0,
    })
    await expect(result).resolves.toBe(0)
  })

  it('an activation that awaits longer than the invocation timeout keeps its depth for the calls it makes after', async () => {
    fake.fakeClock()
    fake.installFakeEngine()
    sdk = registerWorker('ws://context.test', { workerName: 'ui', invocationTimeoutMs: 1000 })
    const iii = sdk
    const socket = fake.lastSocket()
    socket.open()
    socket.clear()
    let release!: () => void
    iii.registerFunction('ui::slow-hop', async () => {
      await new Promise<void>((r) => {
        release = r
      })
      void iii.call({ function_id: 'x::call-back', payload: {}, timeoutMs: 20 }).catch(() => undefined)
      return null
    })
    socket.deliver(inbound('ui::slow-hop', 'hop-1', 5))
    await settle()
    await fake.elapse(5000) // well past invocationTimeoutMs
    release()
    await settle()
    expect(sent(socket, 'x::call-back')).toMatchObject({ observe_depth: 5, caller: { invocation_id: 'hop-1' } })
  })

  it('an invocation that never settles is forgotten after MAX_RUNNING_INVOCATION_MS', async () => {
    fake.fakeClock()
    const { sdk, socket } = worker()
    sdk.registerFunction('ui::stuck', () => new Promise(() => undefined))
    socket.deliver(inbound('ui::stuck', 'stuck-1', 5))
    await settle()
    void sdk.call({ function_id: 'x::while-stuck', payload: {}, timeoutMs: 20 }).catch(() => undefined)
    expect(sent(socket, 'x::while-stuck')).toMatchObject({ observe_depth: 5 })
    await fake.elapse(MAX_RUNNING_INVOCATION_MS - 1000)
    void sdk.call({ function_id: 'x::still-stuck', payload: {}, timeoutMs: 20 }).catch(() => undefined)
    expect(sent(socket, 'x::still-stuck')).toMatchObject({ observe_depth: 5 })
    await fake.elapse(1001)
    void sdk.call({ function_id: 'x::later', payload: {}, timeoutMs: 20 }).catch(() => undefined)
    expect(sent(socket, 'x::later')).not.toHaveProperty('observe_depth')
  })

  it('calls started together outside any handler are siblings: 70 at once all run locally, connected or not', async () => {
    const { sdk, socket } = worker()
    let ran = 0
    let gate!: Promise<void>
    let open!: () => void
    const closeGate = () => {
      gate = new Promise<void>((r) => {
        open = r
      })
    }
    sdk.registerFunction('fan::out', async (n: number) => {
      ran++
      await gate
      return n
    })
    socket.deliver(functionRegistered('fan::out'))
    await settle()
    socket.clear()
    for (const connected of [true, false]) {
      if (!connected) socket.drop()
      closeGate()
      ran = 0
      const calls = Array.from({ length: 70 }, (_, i) => sdk.call({ function_id: 'fan::out', payload: i }))
      expect(ran).toBe(70)
      open()
      await expect(Promise.all(calls)).resolves.toHaveLength(70)
    }
    expect(invokes(socket)).toEqual([])
  })

  it('calls started in separate ticks while earlier ones run are counted as nested: some go remote, none while offline', async () => {
    // A documented limitation of the synchronous scope: unrelated long-running
    // local calls look like a chain to calls made later outside any handler.
    fake.fakeClock()
    const { sdk, socket } = worker()
    let ran = 0
    let open!: () => void
    let gate!: Promise<void>
    const closeGate = () => {
      gate = new Promise<void>((r) => {
        open = r
      })
    }
    sdk.registerFunction('fan::ticks', async (n: number) => {
      ran++
      await gate
      return n
    })
    socket.deliver(functionRegistered('fan::ticks'))
    await settle()
    socket.clear()
    for (const connected of [true, false]) {
      if (!connected) socket.drop()
      closeGate()
      ran = 0
      const calls: Promise<unknown>[] = []
      for (let i = 0; i < 70; i++) {
        calls.push(sdk.call({ function_id: 'fan::ticks', payload: i, timeoutMs: 1000 }).catch(() => 'remote'))
        await settle(1) // the next call starts in a later macrotask
      }
      const remote = connected ? invokes(socket).filter((f) => f.function_id === 'fan::ticks').length : 0
      if (connected) {
        expect(ran).toBe(64)
        expect(remote).toBe(6)
      } else {
        expect(ran).toBe(70) // the engine cannot get them: they run here
      }
      open()
      for (const f of invokes(socket).filter((f) => f.function_id === 'fan::ticks')) {
        socket.deliver({
          type: 'invocationresult',
          invocation_id: f.invocation_id,
          function_id: 'fan::ticks',
          result: 0,
        })
      }
      await Promise.all(calls)
      socket.clear()
    }
  })

  it('a local call stops counting as running at its own timeout', async () => {
    fake.fakeClock()
    const { sdk, socket } = worker()
    let ran = 0
    sdk.registerFunction('hang::fn', async () => {
      ran++
      await new Promise(() => undefined)
    })
    socket.deliver(functionRegistered('hang::fn'))
    await settle()
    socket.clear()
    for (let i = 0; i < 70; i++) {
      void sdk.call({ function_id: 'hang::fn', payload: i, timeoutMs: 20 }).catch(() => undefined)
      await settle(1)
    }
    expect(ran).toBe(64) // the rest were counted as nested (connected: through the engine)
    await fake.elapse(21) // every one timed out: none counts as running any more
    void sdk.call({ function_id: 'hang::fn', payload: 'again', timeoutMs: 20 }).catch(() => undefined)
    expect(ran).toBe(65)
    // Let that one expire too, and have a call prune it, so no lease outlives the fake clock.
    await fake.elapse(21)
    void sdk.call({ function_id: 'x::prune', payload: {}, timeoutMs: 20 }).catch(() => undefined)
  })

  it('a recursion through copied withInvocation views still reaches the engine at 64 local calls', async () => {
    const { sdk, socket } = worker()
    let local = 0
    sdk.registerFunction('rec::copied', async (n: number): Promise<number> => {
      local++
      const inv = JSON.parse(JSON.stringify(sdk.currentInvocation())) as InvocationContext
      await Promise.resolve()
      return n === 0
        ? 0
        : sdk.withInvocation(inv, () => sdk.call<number, number>({ function_id: 'rec::copied', payload: n - 1 }))
    })
    socket.deliver(functionRegistered('rec::copied'))
    await settle()
    socket.clear()
    const result = sdk.call({ function_id: 'rec::copied', payload: 300 })
    await settle()
    expect(local).toBe(64)
    const [remote] = invokes(socket)
    expect(remote).toMatchObject({ function_id: 'rec::copied', data: 236 })
    socket.deliver({
      type: 'invocationresult',
      invocation_id: remote.invocation_id,
      function_id: 'rec::copied',
      result: 0,
    })
    await expect(result).resolves.toBe(0)
  })

  it('a recursion through one stored withInvocation view still reaches the engine at 64 local calls', async () => {
    const { sdk, socket } = worker()
    let local = 0
    let root: InvocationContext | undefined
    sdk.registerFunction('rec::stored', async (n: number): Promise<number> => {
      local++
      root ??= sdk.currentInvocation() // the first level's view, entered again by every level
      await Promise.resolve()
      return n === 0
        ? 0
        : sdk.withInvocation(root, () => sdk.call<number, number>({ function_id: 'rec::stored', payload: n - 1 }))
    })
    socket.deliver(functionRegistered('rec::stored'))
    await settle()
    socket.clear()
    const result = sdk.call({ function_id: 'rec::stored', payload: 300 })
    await settle()
    expect(local).toBe(64)
    const [remote] = invokes(socket)
    expect(remote).toMatchObject({ function_id: 'rec::stored', data: 236 })
    socket.deliver({
      type: 'invocationresult',
      invocation_id: remote.invocation_id,
      function_id: 'rec::stored',
      result: 0,
    })
    await expect(result).resolves.toBe(0)
  })

  it.each([
    ['awaits before each call', true],
    ['calls synchronously', false],
  ])('offline, a self-recursion that %s stops at MAX_OFFLINE_LOCAL_DEPTH, with nothing unhandled', async (_name, awaits) => {
    const { sdk, socket } = worker()
    let local = 0
    sdk.registerFunction('rec::offline', async (n: number): Promise<number> => {
      local++
      if (awaits) await Promise.resolve()
      return n === 0 ? 0 : sdk.call<number, number>({ function_id: 'rec::offline', payload: n - 1 })
    })
    socket.deliver(functionRegistered('rec::offline'))
    await settle()
    socket.drop()
    socket.clear()
    let failure: unknown
    const unhandled = await unhandledDuring(async () => {
      failure = await sdk.call({ function_id: 'rec::offline', payload: 5000 }).then(
        () => undefined,
        (error: unknown) => error,
      )
    })
    expect(local).toBe(MAX_OFFLINE_LOCAL_DEPTH)
    // The innermost call failed at once; every level above failed with it, as handler failures do.
    expect(failure).toMatchObject({
      code: 'invocation_failed',
      message: expect.stringContaining('LOCAL_DEPTH_EXCEEDED'),
    })
    expect(unhandled).toEqual([])
    expect(socket.sent).toHaveLength(0)
  })

  it('a stored view re-entered deeper is a separate entry: it leaves the stored invocation as deep as it was', async () => {
    const { sdk, socket } = worker()
    let stored: InvocationContext | undefined
    let releaseHold!: () => void
    sdk.registerFunction('ui::hold', async () => {
      stored = sdk.currentInvocation() // a pending remote invocation stores its view
      await new Promise<void>((r) => {
        releaseHold = r
      })
      return null
    })
    sdk.registerFunction('rec::bottom', async (n: number): Promise<unknown> => {
      if (n === 0) {
        // 64 local calls deep: enter the stored view here, with work of its own
        return sdk.withInvocation(stored, async () => {
          await Promise.resolve()
          return 'bottom'
        })
      }
      await Promise.resolve()
      return sdk.call({ function_id: 'rec::bottom', payload: n - 1 })
    })
    sdk.registerFunction('ui::plain', async () => 'plain')
    socket.deliver(inbound('ui::hold', 'hold-1', 0))
    socket.deliver(functionRegistered('rec::bottom'))
    socket.deliver(functionRegistered('ui::plain'))
    await settle()
    socket.clear()
    await expect(sdk.call({ function_id: 'rec::bottom', payload: 63 })).resolves.toBe('bottom')
    // ui::hold still runs; a later unscoped local call is not counted 64 deep
    await expect(sdk.call({ function_id: 'ui::plain', payload: {}, timeoutMs: 100 })).resolves.toBe('plain')
    expect(socket.sent).toHaveLength(0)
    releaseHold()
  })

  it('a late settle of expired work does not end the invocation entered again since', async () => {
    fake.fakeClock()
    const { sdk, socket } = worker()
    let inv: InvocationContext | undefined
    sdk.registerFunction('ui::entry', async () => {
      inv = sdk.currentInvocation()
      return null
    })
    socket.deliver(inbound('ui::entry', 'entry-1', 4))
    await settle()
    const gate = () => {
      let open!: () => void
      const promise = new Promise<void>((r) => {
        open = r
      })
      return { promise, open }
    }
    const first = gate()
    const done = sdk.withInvocation(inv, () => first.promise)
    await fake.elapse(MAX_RUNNING_INVOCATION_MS + 1) // the first lease expired
    const second = gate()
    void sdk.withInvocation(inv, () => second.promise)
    first.open() // its late settle ends only its own lease
    await done
    void sdk.call({ function_id: 'x::still-entered', payload: {}, timeoutMs: 20 }).catch(() => undefined)
    expect(sent(socket, 'x::still-entered')).toMatchObject({ observe_depth: 4, caller: { invocation_id: 'entry-1' } })
    second.open()
  })

  it("a source called after an await carries the running activation's observe_depth, and is that invocation", async () => {
    fake.fakeClock()
    const { sdk, socket } = worker()
    sdk.registerFunction('ui::source', (event: Record<string, unknown>) => event, { private: true })
    sdk.registerFunction('ui::on-change', async (event: Record<string, unknown>) => {
      await Promise.resolve()
      await sdk.call({ function_id: 'ui::source', payload: event })
      return null
    })
    socket.deliver(
      functionRegistered('ui::source', [observerInfo('o-1', 't::a', { capture: ['input', 'output', 'caller'] })]),
    )
    await settle()
    socket.clear()
    socket.deliver(inbound('ui::on-change', 'act-1', 4, { data: { n: 1 } }))
    await settle()
    await flushed()
    expect(reportedItems(socket)).toEqual([
      expect.objectContaining({
        matched: ['o-1'],
        input: { n: 1 },
        output_is_input: true,
        observe_depth: 4,
        caller: expect.objectContaining({ function_id: 'ui::on-change', invocation_id: 'act-1' }),
      }),
    ])
  })
})

describe('a deliver hook after an await', () => {
  /** `ui::on-command`: its deliver hook awaits `gate`, then calls through the observer its command names. */
  async function commands(gate: () => Promise<void>) {
    const { sdk, socket } = worker()
    const routes = new Map<string, Observer>()
    const seen: (InvocationContext | undefined)[] = []
    sdk
      .registerFunction('ui::on-command', (command: { name: string }) => command, { private: true })
      .onObserverAdded((obs) => {
        routes.set(obs.args.name as string, obs)
      })
      .onObserverDeliver(async (call) => {
        seen.push(sdk.currentInvocation())
        await gate()
        const obs = routes.get(call.input.name)
        if (!obs) return { handled: false }
        void sdk.call({ function_id: 'x::audit', payload: call.input, timeoutMs: 20 }).catch(() => undefined)
        return sdk.call({ observer: obs, payload: call.input })
      })
    socket.deliver(
      functionRegistered('ui::on-command', [observerInfo('save', 'editor::save', { args: { name: 'save' } })]),
    )
    await settle()
    socket.clear()
    return { sdk, socket, seen }
  }

  it("the calls it makes after the await keep its invocation, while it is the only one running; a call through an observer goes to the observer's target", async () => {
    const { socket, seen } = await commands(() => Promise.resolve())
    socket.deliver(
      inbound('ui::on-command', 'cmd-1', 0, {
        data: { name: 'save' },
        observe_depth: 2,
        traceparent: TRACEPARENT,
        caller: { worker_id: 'w-0', worker_name: 'shell', function_id: 'shell::key', invocation_id: 'k-1' },
      }),
    )
    await settle()
    expect(seen.map((inv) => inv?.invocation_id)).toEqual(['cmd-1'])
    const caller = {
      function_id: 'ui::on-command',
      invocation_id: 'cmd-1',
      parent: { worker_id: 'w-0', worker_name: 'shell', function_id: 'shell::key', invocation_id: 'k-1' },
    }
    expect(sent(socket, 'x::audit')).toMatchObject({ caller, observe_depth: 2, traceparent: TRACEPARENT })
    const target = sent(socket, 'editor::save') as Record<string, unknown>
    expect(target).toMatchObject({
      function_id: 'editor::save',
      data: { name: 'save' },
      activation: { observer_id: 'save' },
      caller,
      observe_depth: 2,
      traceparent: TRACEPARENT,
    })
    expect(target).not.toHaveProperty('namespace')
    socket.deliver({
      type: 'invocationresult',
      invocation_id: target.invocation_id,
      function_id: 'editor::save',
      result: { saved: true },
    })
    await settle()
    // the hook's answer is the target's; a function with a deliver hook lists no `observed`
    expect(fake.resultOf(socket, 'cmd-1')).toEqual({
      type: 'invocationresult',
      invocation_id: 'cmd-1',
      function_id: 'ui::on-command',
      result: { saved: true },
      traceparent: TRACEPARENT,
    })
  })

  it('with two deliveries running, a call after the await carries the deepest depth and no caller; the call through the observer is routed all the same', async () => {
    const gates: (() => void)[] = []
    const { socket } = await commands(() => new Promise<void>((r) => gates.push(r)))
    socket.deliver(inbound('ui::on-command', 'cmd-1', 0, { data: { name: 'save' }, observe_depth: 1 }))
    socket.deliver(inbound('ui::on-command', 'cmd-2', 0, { data: { name: 'save' }, observe_depth: 3 }))
    await settle()
    gates[0]()
    await settle()
    const audit = sent(socket, 'x::audit')
    expect(audit).toMatchObject({ observe_depth: 3 })
    expect(audit).not.toHaveProperty('caller')
    expect(sent(socket, 'editor::save')).toMatchObject({
      function_id: 'editor::save',
      activation: { observer_id: 'save' },
      observe_depth: 3,
    })
    gates[1]()
    await settle()
  })

  it('once the hook settled, nothing falls back to its invocation', async () => {
    const { sdk, socket } = await commands(() => Promise.resolve())
    socket.deliver(inbound('ui::on-command', 'cmd-1', 0, { data: { name: 'none' }, observe_depth: 2 }))
    await settle()
    expect(fake.resultOf(socket, 'cmd-1')).toMatchObject({ result: { handled: false } })
    void sdk.call({ function_id: 'x::later', payload: {}, timeoutMs: 20 }).catch(() => undefined)
    const later = sent(socket, 'x::later')
    expect(later).not.toHaveProperty('caller')
    expect(later).not.toHaveProperty('observe_depth')
  })

  it('on the local path too: the hook of a local call keeps the local invocation after its await', async () => {
    const { sdk, socket } = await commands(() => Promise.resolve())
    socket.deliver(
      functionRegistered('ui::on-command', [observerInfo('save', 'editor::save', { args: { name: 'save' } })]),
    )
    await settle()
    socket.clear()
    const response = sdk.call({ function_id: 'ui::on-command', payload: { name: 'save' } })
    await settle()
    const target = sent(socket, 'editor::save') as Record<string, unknown>
    expect(target).toMatchObject({
      activation: { observer_id: 'save' },
      caller: {
        function_id: 'ui::on-command',
        invocation_id: expect.any(String),
        parent: { worker_id: 'worker-1', worker_name: 'ui' },
      },
    })
    socket.deliver({
      type: 'invocationresult',
      invocation_id: target.invocation_id,
      function_id: 'editor::save',
      result: { saved: true },
    })
    await expect(response).resolves.toEqual({ saved: true })
  })
})

describe('what a handler failure reports (the same on the remote and the local path)', () => {
  class Custom {
    code = 'E_CUSTOM'
    message = 'custom message'
    toString() {
      return 'Custom: via toString'
    }
  }
  const getterThrows = Object.defineProperty({ code: 'E_GETTER' }, 'message', {
    enumerable: true,
    get() {
      throw new Error('getter throws')
    },
  })
  const errorGetterThrows = Object.defineProperty(new Error('hidden'), 'message', {
    get() {
      throw new Error('getter throws')
    },
  })
  const UNREADABLE = 'the handler failed with a value that cannot be read'
  const cases: [string, () => unknown, string, boolean][] = [
    ['an Error', () => new Error('boom'), 'boom', true],
    ['an Error with a code', () => Object.assign(new Error('coded'), { code: 'E_CODED' }), 'coded', true],
    ['a string', () => 'plain string', 'plain string', false],
    ['a number', () => 42, '42', false],
    ['null', () => null, 'null', false],
    ['undefined', () => undefined, 'undefined', false],
    ['an engine error body', () => ({ code: 'E_ENGINE', message: 'from the engine' }), 'from the engine', false],
    [
      'an engine error body with a stacktrace',
      () => ({ code: 'E_ENGINE', message: 'with a trace', stacktrace: 'at somewhere' }),
      'with a trace',
      true,
    ],
    ['a class instance with code and message (not a body)', () => new Custom(), 'Custom: via toString', false],
    ['an Error from another realm', () => runInNewContext("new Error('other realm')"), 'other realm', true],
    ['an object with a message only', () => ({ message: 'no code' }), '[object Object]', false],
    ['an object whose code is a number', () => ({ code: 7, message: 'numeric code' }), '[object Object]', false],
    ['an object whose message getter throws', () => getterThrows, '[object Object]', false],
    ['an Error whose message getter throws', () => errorGetterThrows, UNREADABLE, false],
  ]

  it.each(cases)('%s', async (_name, thrown, message, hasStack) => {
    const { sdk, socket } = worker()
    sdk.registerFunction('ui::fails', async () => {
      throw thrown()
    })
    // remote: the engine invokes it, and always gets an answer
    socket.deliver({ type: 'invokefunction', invocation_id: 'fail-1', function_id: 'ui::fails', data: {} })
    await settle()
    const [result] = socket.framesOf('invocationresult')
    expect(result?.error).toEqual({
      code: 'invocation_failed',
      message,
      ...(hasStack ? { stacktrace: expect.any(String) } : {}),
    })
    // local: the caller's rejection carries the same body
    socket.deliver(functionRegistered('ui::fails'))
    const local = await sdk.call({ function_id: 'ui::fails', payload: {} }).then(
      () => undefined,
      (error: unknown) => error,
    )
    expect(local).toMatchObject({ code: 'invocation_failed', message })
    expect(typeof (local as { stacktrace?: unknown }).stacktrace).toBe(hasStack ? 'string' : 'undefined')
  })
})

describe('withInvocation', () => {
  it('re-enters a captured invocation after an await: caller, observe_depth and traceparent', async () => {
    const { sdk, socket } = worker()
    let inner: InvocationContext | undefined
    let returned: unknown
    sdk.registerFunction('ui::checkout', async () => {
      const inv = sdk.currentInvocation()
      await Promise.resolve()
      returned = sdk.withInvocation(inv, () => {
        inner = sdk.currentInvocation()
        void sdk.call({ function_id: 'orders::create', payload: {}, timeoutMs: 20 }).catch(() => undefined)
        return 'value of fn'
      })
      return null
    })
    socket.deliver({
      type: 'invokefunction',
      invocation_id: 'i-1',
      function_id: 'ui::checkout',
      data: {},
      caller: {
        worker_id: 'w-0',
        worker_name: 'shell',
        function_id: 'shell::start',
        invocation_id: 'i-0',
      },
      observe_depth: 2,
      traceparent: TRACEPARENT,
      activation: { observer_id: 'o-1', activation_id: 'a-1' },
    })
    await settle()
    expect(returned).toBe('value of fn')
    expect(inner).toEqual({
      function_id: 'ui::checkout',
      invocation_id: 'i-1',
      caller: {
        worker_id: 'w-0',
        worker_name: 'shell',
        function_id: 'shell::start',
        invocation_id: 'i-0',
      },
      activation: { observer_id: 'o-1', activation_id: 'a-1' },
      observe_depth: 2,
      traceparent: TRACEPARENT,
    })
    expect(sent(socket, 'orders::create')).toMatchObject({
      caller: {
        function_id: 'ui::checkout',
        invocation_id: 'i-1',
        parent: {
          worker_id: 'w-0',
          worker_name: 'shell',
          function_id: 'shell::start',
          invocation_id: 'i-0',
        },
      },
      observe_depth: 2,
      traceparent: TRACEPARENT,
    })
    expect(sdk.currentInvocation()).toBeUndefined()
  })

  it('a source called inside it is reported with that invocation as its caller; outside it, without one', async () => {
    fake.fakeClock()
    const { sdk, socket } = worker()
    sdk.registerFunction('ui::clicked', (event: Record<string, unknown>) => event, { private: true })
    sdk.registerFunction('ui::on-click', async (event: { button: string }) => {
      const inv = sdk.currentInvocation()
      await Promise.resolve()
      await sdk.withInvocation(inv, () =>
        sdk.call({ function_id: 'ui::clicked', payload: event, metadata: { page: 'cart' } }),
      )
      await sdk.call({ function_id: 'ui::clicked', payload: { ...event, late: true } }) // outside: filtered out by metadata
      return null
    })
    socket.deliver(
      functionRegistered('ui::clicked', [
        observerInfo('o-1', 't::a', {
          capture: ['input', 'caller', 'metadata'],
          filters: { $and: [{ 'metadata.page': { $eq: 'cart' } }] },
        }),
        observerInfo('o-2', 't::b', { capture: ['input', 'caller'] }),
      ]),
    )
    await settle()
    socket.clear()
    socket.deliver({
      type: 'invokefunction',
      invocation_id: 'click-1',
      function_id: 'ui::on-click',
      data: { button: 'buy' },
      caller: { worker_id: 'w-0', worker_name: 'shell' },
    })
    await settle()
    await flushed()
    const [inside, outside] = reportedItems(socket)
    expect(inside).toMatchObject({
      matched: ['o-1', 'o-2'],
      input: { button: 'buy' },
      metadata: { page: 'cart' },
      caller: {
        function_id: 'ui::on-click',
        invocation_id: 'click-1',
        parent: { worker_id: 'w-0', worker_name: 'shell' },
      },
    })
    expect(outside).toMatchObject({ matched: ['o-2'], input: { button: 'buy', late: true } })
    expect(outside).not.toHaveProperty('caller')
  })

  it('accepts a context built by hand, and undefined runs outside any invocation', async () => {
    const { sdk, socket } = worker()
    const built: InvocationContext = {
      function_id: 'ui::root',
      invocation_id: 'r-1',
      observe_depth: 1,
    }
    sdk.withInvocation(built, () => {
      expect(sdk.currentInvocation()).toEqual(built)
      void sdk.call({ function_id: 'x::from-built', payload: {}, timeoutMs: 20 }).catch(() => undefined)
      sdk.withInvocation(undefined, () => {
        expect(sdk.currentInvocation()).toBeUndefined()
        void sdk.call({ function_id: 'x::detached', payload: {}, timeoutMs: 20 }).catch(() => undefined)
      })
      expect(sdk.currentInvocation()?.invocation_id).toBe('r-1')
    })
    expect(sent(socket, 'x::from-built')).toMatchObject({
      caller: { function_id: 'ui::root', invocation_id: 'r-1' },
      observe_depth: 1,
    })
    expect(sent(socket, 'x::detached')).not.toHaveProperty('caller')
    expect(() => sdk.withInvocation({} as InvocationContext, () => undefined)).toThrow(/withInvocation/)
  })

  it('with an async fn: the invocation lasts until its promise settles, which is what it returns', async () => {
    const { sdk, socket } = worker()
    const built: InvocationContext = {
      function_id: 'ui::root',
      invocation_id: 'r-1',
      observe_depth: 2,
      activation: { observer_id: 'o-1' },
    }
    let release!: () => void
    const result = sdk.withInvocation(built, async () => {
      void sdk.call({ function_id: 'x::before-await', payload: {}, timeoutMs: 20 }).catch(() => undefined)
      await new Promise<void>((r) => {
        release = r
      })
      void sdk.call({ function_id: 'x::after-await', payload: {}, timeoutMs: 20 }).catch(() => undefined)
      return 'done'
    })
    expect(result).toBeInstanceOf(Promise)
    expect(sdk.currentInvocation()).toBeUndefined()
    release()
    await expect(result).resolves.toBe('done')
    for (const id of ['x::before-await', 'x::after-await']) {
      expect(sent(socket, id)).toMatchObject({
        caller: { function_id: 'ui::root', invocation_id: 'r-1' },
        observe_depth: 2,
      })
    }
    // settled: no longer running, so a later call is top-level again
    await Promise.resolve()
    void sdk.call({ function_id: 'x::later', payload: {}, timeoutMs: 20 }).catch(() => undefined)
    expect(sent(socket, 'x::later')).not.toHaveProperty('observe_depth')
  })

  it('with an async fn that rejects: the caller gets the rejection, and one nobody handles is reported', async () => {
    const { sdk, socket } = worker()
    const built: InvocationContext = {
      function_id: 'ui::root',
      invocation_id: 'r-1',
      observe_depth: 3,
      activation: { observer_id: 'o-1' },
    }
    await expect(
      sdk.withInvocation(built, async () => {
        throw new Error('awaited')
      }),
    ).rejects.toThrow('awaited')
    // once it rejected it is no longer running: an unscoped call is top-level again
    void sdk.call({ function_id: 'x::after-rejection', payload: {}, timeoutMs: 20 }).catch(() => undefined)
    expect(sent(socket, 'x::after-rejection')).not.toHaveProperty('observe_depth')
    expect(sent(socket, 'x::after-rejection')).not.toHaveProperty('caller')

    // Swap vitest's own listeners out while a deliberately unhandled rejection is observed.
    const reported: unknown[] = []
    const vitestListeners = process.listeners('unhandledRejection')
    process.removeAllListeners('unhandledRejection')
    const listener = (reason: unknown) => {
      reported.push(reason)
    }
    process.on('unhandledRejection', listener)
    try {
      void sdk.withInvocation(built, async () => {
        throw new Error('nobody awaits')
      })
      for (let i = 0; i < 3; i++) await new Promise<void>((r) => setImmediate(r))
    } finally {
      process.off('unhandledRejection', listener)
      for (const l of vitestListeners) process.on('unhandledRejection', l)
    }
    expect(reported).toEqual([expect.objectContaining({ message: 'nobody awaits' })])
  })

  it('restores the outer scope when fn throws', () => {
    const { sdk } = worker()
    const built: InvocationContext = {
      function_id: 'ui::root',
      invocation_id: 'r-1',
      observe_depth: 0,
    }
    expect(() =>
      sdk.withInvocation(built, () => {
        throw new Error('inside')
      }),
    ).toThrow('inside')
    expect(sdk.currentInvocation()).toBeUndefined()
  })
})
