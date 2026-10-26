import type { ActivationWire, CallerWire } from './iii-types'
import type { InvocationContext } from './observe-types'

/**
 * The invocation a handler runs in. Set around every handler, remote
 * (`InvokeFunction`) and local (in-process dispatch), and read by outgoing
 * calls (`caller`, `observe_depth`, `traceparent`) and by `obs.emit` /
 * `obs.call`.
 */
export type ActiveInvocation = {
  function_id: string
  invocation_id: string
  caller?: CallerWire
  activation?: ActivationWire
  observe_depth: number
  /** The invocation's metadata sidecar: what an emitted event's filters see as `metadata`. */
  metadata?: unknown
  /** The trace context the invocation arrived with: what calls and events made from it carry. */
  traceparent?: string
  baggage?: string
  /** How many local calls deep this invocation is (0 for one that came over the wire). */
  localDepth?: number
}

/**
 * A local call nested deeper than this goes to the engine instead, so
 * recursion cannot overflow the stack. (Nested local calls made before the
 * first `await` all run on one JavaScript stack; after an `await` the stack
 * starts over, and so does the scope.)
 */
export const MAX_LOCAL_DEPTH = 64

/**
 * While the socket is closed, a local call past {@link MAX_LOCAL_DEPTH} cannot
 * go to the engine: it runs here up to this depth (above any realistic
 * fan-out counted as nested), and past it fails at once with
 * `LOCAL_DEPTH_EXCEEDED`, so a runaway recursion neither overflows the stack
 * nor runs forever.
 */
export const MAX_OFFLINE_LOCAL_DEPTH = 256

/**
 * How long an invocation that came over the wire (and one re-entered with
 * `withInvocation`) counts as running at most, when it never settles. An
 * observer chain's hops may await far longer than this SDK's own call
 * timeout, and their depth must still reach the calls they make afterwards
 * (the engine's loop guard counts on it). A local call counts as running until
 * its caller's timeout instead.
 */
export const MAX_RUNNING_INVOCATION_MS = 10 * 60 * 1000

/**
 * Browsers have no `AsyncLocalStorage` (and `AsyncContext` is not shipped
 * yet), so the context is a synchronous scope: {@link runInContext} sets it
 * while `fn` runs and restores the previous one when `fn` returns. An async
 * handler sees it up to its first `await`; the continuation after an `await`
 * runs from the event loop, outside any scope, so it never sees another
 * invocation's context, only none. `withInvocation` re-enters a captured one.
 *
 * One scope for the whole page, like the Node SDK's one `AsyncLocalStorage`
 * per process: a handler of one worker that calls another worker's
 * `trigger()` is that call's caller.
 */
let current: ActiveInvocation | undefined

/** The public views handed out, so `withInvocation(view, fn)` restores everything (metadata included). */
const views = new WeakMap<object, ActiveInvocation>()

export function currentContext(): ActiveInvocation | undefined {
  return current
}

/**
 * Every invocation whose work is running on this page (handlers, remote and
 * local, and the async `fn` of `withInvocation`), until it settles: one lease
 * per piece of work, because an async `withInvocation` keeps an invocation
 * running alongside its handler. A lease is forgotten at its deadline, so work
 * that never settles does not lend its depth to every later call made on the
 * page; settling one lease never ends another (a late one, after its deadline
 * passed and the invocation was entered again, ends only itself).
 */
const running = new Map<ActiveInvocation, Set<{ deadline: number }>>()

/**
 * Marks `ctx` as running for at most `maxAgeMs`; call the returned function
 * when the work settles.
 */
export function trackRunning(ctx: ActiveInvocation, maxAgeMs: number): () => void {
  const lease = { deadline: Date.now() + maxAgeMs }
  let leases = running.get(ctx)
  if (!leases) {
    leases = new Set()
    running.set(ctx, leases)
  }
  leases.add(lease)
  return () => {
    const current = running.get(ctx)
    if (!current?.delete(lease)) return
    if (current.size === 0) running.delete(ctx)
  }
}

/** The running invocations; leases past their deadline are forgotten here. */
function live(): ActiveInvocation[] {
  const now = Date.now()
  const out: ActiveInvocation[] = []
  for (const [ctx, leases] of running) {
    for (const lease of leases) if (lease.deadline <= now) leases.delete(lease)
    if (leases.size === 0) running.delete(ctx)
    else out.push(ctx)
  }
  return out
}

/** What a fallback attribution lends: the invocation without its metadata sidecar (one object per invocation). */
const lent = new WeakMap<ActiveInvocation, ActiveInvocation>()

function lend(ctx: ActiveInvocation): ActiveInvocation {
  let view = lent.get(ctx)
  if (!view) {
    const { metadata: _, ...rest } = ctx
    view = rest
    lent.set(ctx, view)
  }
  return view
}

/**
 * The invocation a call, an emit or a report made now belongs to. Inside a
 * scope, the scope's. Outside any scope (code after an `await`), the running
 * invocation when that is unambiguous: it is the only one running and it is
 * an observe activation. It lends its ids, caller and trace context, never its
 * metadata: filters on `metadata.*` see only what a real scope carries.
 * Otherwise none, and the call carries no `caller`: the SDK cannot tell which
 * handler, if any, made it.
 */
export function callContext(): ActiveInvocation | undefined {
  if (current) return current
  const all = live()
  return all.length === 1 && all[0].activation ? lend(all[0]) : undefined
}

/**
 * The `observe_depth` a call, an emit or a report made now carries. Inside a
 * scope, the scope's. Outside any scope, the deepest running invocation's:
 * the engine's loop guard counts on this depth, so code after an `await`
 * over-counts (a chain may stop earlier than `max_observe_depth`) rather than
 * restart the chain at 0 (an observer cycle would never stop).
 */
export function callDepth(): number {
  if (current) return current.observe_depth
  let depth = 0
  for (const ctx of live()) if (ctx.observe_depth > depth) depth = ctx.observe_depth
  return depth
}

/**
 * The local depth that calls started outside any scope in the current
 * synchronous run share (see {@link callLocalDepth}); cleared on a microtask.
 */
let unscopedLocalDepth: number | undefined

/**
 * How many local calls deep a local call made now is nested (decision 20,
 * {@link MAX_LOCAL_DEPTH}). Inside a scope, the scope's. Outside any scope, the
 * deepest running local call's, so a self-recursion that awaits before each
 * call still reaches the engine at the limit. That value is taken once per
 * synchronous run: calls started together outside any scope (a fan-out, from
 * a loop or `Promise.all`) are siblings at one depth; only code after an
 * `await`, which runs later, sees them running and goes one level deeper.
 */
export function callLocalDepth(): number {
  if (current) return current.localDepth ?? 0
  if (unscopedLocalDepth === undefined) {
    let depth = 0
    for (const ctx of live()) if ((ctx.localDepth ?? 0) > depth) depth = ctx.localDepth ?? 0
    unscopedLocalDepth = depth
    queueMicrotask(() => {
      unscopedLocalDepth = undefined
    })
  }
  return unscopedLocalDepth
}

/**
 * `withInvocation`: runs `fn` inside `ctx`. When `fn` returns a promise, `ctx`
 * counts as running until it settles (at most `maxAgeMs`), so `fn`'s code after
 * its own `await`s falls back to it (see {@link callContext}, {@link callDepth}),
 * and the caller gets a promise that settles the same way: a rejection the
 * caller does not handle is still reported as unhandled.
 */
export function enterInvocation<T>(ctx: ActiveInvocation | undefined, fn: () => T, maxAgeMs: number): T {
  const result = runInContext(ctx, fn)
  if (!ctx || result === null || typeof (result as { then?: unknown })?.then !== 'function') return result
  const untrack = trackRunning(ctx, maxAgeMs)
  return (result as PromiseLike<unknown>).then(
    (value) => {
      untrack()
      return value
    },
    (error) => {
      untrack()
      throw error
    },
  ) as T
}

/** Runs `fn` synchronously inside `ctx` (`undefined`: outside any invocation). */
export function runInContext<T>(ctx: ActiveInvocation | undefined, fn: () => T): T {
  const previous = current
  current = ctx
  try {
    return fn()
  } finally {
    current = previous
  }
}

/** The public view of the current invocation (without the metadata sidecar). */
export function publicContext(ctx: ActiveInvocation | undefined): InvocationContext | undefined {
  if (!ctx) return undefined
  const view: InvocationContext = {
    function_id: ctx.function_id,
    invocation_id: ctx.invocation_id,
    observe_depth: ctx.observe_depth,
  }
  if (ctx.caller) view.caller = ctx.caller
  if (ctx.activation) view.activation = ctx.activation
  if (ctx.traceparent !== undefined) view.traceparent = ctx.traceparent
  if (ctx.baggage !== undefined) view.baggage = ctx.baggage
  views.set(view, ctx)
  return view
}

/**
 * The context `withInvocation` re-enters: the one a view of
 * `currentInvocation()` was taken from, or one built from a context the
 * caller assembled itself or copied (at the local depth of the code entering it).
 */
export function contextFromView(view: InvocationContext | undefined): ActiveInvocation | undefined {
  if (view === undefined || view === null) return undefined
  const known = views.get(view)
  if (known) {
    // Stored and entered again later, deeper in local calls: as deep as the
    // code entering it, as its own invocation entry, so a recursion through
    // one stored view still reaches the engine at MAX_LOCAL_DEPTH.
    const depth = callLocalDepth()
    return depth > (known.localDepth ?? 0) ? { ...known, localDepth: depth } : known
  }
  if (typeof view.function_id !== 'string' || typeof view.invocation_id !== 'string') {
    throw new Error('withInvocation: expected the value iii.currentInvocation() returned')
  }
  const ctx: ActiveInvocation = {
    function_id: view.function_id,
    invocation_id: view.invocation_id,
    observe_depth: typeof view.observe_depth === 'number' && view.observe_depth > 0 ? view.observe_depth : 0,
    // A copy (JSON, structuredClone) does not say how deep in local calls it
    // was taken: it is as deep as the code entering it, so a recursion through
    // copied views still reaches the engine at MAX_LOCAL_DEPTH.
    localDepth: callLocalDepth(),
  }
  if (view.caller) ctx.caller = view.caller
  if (view.activation) ctx.activation = view.activation
  if (typeof view.traceparent === 'string') ctx.traceparent = view.traceparent
  if (typeof view.baggage === 'string') ctx.baggage = view.baggage
  return ctx
}
