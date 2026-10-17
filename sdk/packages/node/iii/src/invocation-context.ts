import { AsyncLocalStorage } from 'node:async_hooks'
import type { ActivationWire, CallerWire } from './iii-types'
import type { InvocationContext } from './observe-types'

/**
 * The invocation a handler runs in. Set around every handler, remote
 * (`InvokeFunction`) and local (in-process dispatch), and read by outgoing
 * calls (`caller`, `observe_depth`) and by `obs.emit` / `obs.call`.
 */
export type ActiveInvocation = {
  function_id: string
  invocation_id: string
  caller?: CallerWire
  activation?: ActivationWire
  observe_depth: number
  /** The invocation's metadata sidecar: what an emitted event's filters see as `metadata`. */
  metadata?: unknown
  /** The trace context the invocation arrived with: what an event emitted from it carries. */
  traceparent?: string
  baggage?: string
  /** How many local calls deep this invocation is (0 for one that came over the wire). */
  localDepth?: number
}

/** A local call nested deeper than this goes to the engine instead, so recursion cannot overflow the stack. */
export const MAX_LOCAL_DEPTH = 64

const storage = new AsyncLocalStorage<ActiveInvocation>()

export function currentContext(): ActiveInvocation | undefined {
  return storage.getStore()
}

export function runInContext<T>(ctx: ActiveInvocation, fn: () => T): T {
  return storage.run(ctx, fn)
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
  return view
}
