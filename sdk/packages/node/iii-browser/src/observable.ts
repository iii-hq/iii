// Derived from sdk/packages/node/iii/src/observable.ts by scripts/sync-browser-observe.py; edit those, not this file.
/**
 * The owner side of observe (sdk.md §1, §4.1–§4.3): per-function observer
 * state, the observe hooks, the `Observer` object handed to them, and the
 * reports of local invocations and `obs.emit` events.
 */
import { InvocationError, ObserveRejection } from './errors'
import { type CompiledFilter, compileFilter } from './filter'
import type { CallerWire, ErrorBody, ObserverInfoWire } from './iii-types'
import { type ActiveInvocation, callContext, callDepth } from './invocation-context'
import type { Capture, Filter, ObserveDoc, ObserveHooks, Observer } from './observe-types'
import { stripCaller } from './observe-wire'
import type { Reporter } from './reporter'
import { randomUUID, unrefTimer } from './utils'

/** CONTRACT D11: `onObserverAdded` (and `onObserverRemoved`) still running after this is `OWNER_TIMEOUT`. */
export const OBSERVE_HOOK_TIMEOUT_MS = 30_000

export interface ObserveHost {
  /** This worker, as the engine knows it. */
  identity(): { worker_id?: string; worker_name: string }
  sendAck(ack: {
    function_id: string
    namespace?: string
    id: string
    revision: number
    error?: { code: string; message: string }
  }): void
  readonly reporter: Reporter
  /** `obs.call`: a remote invocation of the target marked with the activation. */
  callTarget(target: string, observerId: string, payload: unknown, timeoutMs?: number): Promise<unknown>
  logError(message: string, error?: unknown): void
  hookTimeoutMs(): number
  /** The trace context an event emitted outside any handler carries (the active span, if any). */
  traceContext(): { traceparent?: string; baggage?: string }
}

type HookOutcome =
  | { ok: true }
  | { ok: false; code: string; message: string; timedOut: boolean; late: Promise<unknown> }

function sameJson(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b)
}

function rejectionBody(error: unknown): { code: string; message: string } {
  if (error instanceof ObserveRejection) return { code: error.code, message: error.message }
  return { code: 'OBSERVE_REJECTED', message: error instanceof Error ? error.message : String(error) }
}

function onAccepts(on: string, ok: boolean): boolean {
  return on === 'complete' || (on === 'success' ? ok : on === 'error' ? !ok : false)
}

/** The `Observer` an owner receives. Its identity is stable while the observer lives. */
export class ObserverImpl implements Observer {
  readonly id: string
  readonly function_id: string
  args: Record<string, unknown>
  filters: Filter
  target: string
  capture: Capture[]
  on: 'success' | 'error' | 'complete'
  legacy?: { trigger_type: string; config: unknown }
  /** @internal */
  filter: CompiledFilter
  /** @internal The revision it was acked with (echoed by a late rejection). */
  revision: number
  /** @internal In the compiled set: `emit` / `call` deliver to it. */
  active = false
  /**
   * @internal The owner's late rejection (`obs.reject`). It survives a
   * reconnect: a replayed set that lists the observer is acked with it again,
   * and only a fresh `observeradded` reactivates the observer.
   */
  rejection?: { code: string; message: string }
  /** @internal Its `onObserverAdded` is running: a `reject()` now is applied by the add, not sent. */
  adding = false

  constructor(
    private readonly owner: FunctionObservers,
    info: ObserverInfoWire,
    filter: CompiledFilter,
    revision: number,
  ) {
    this.id = info.id
    this.function_id = owner.functionId
    this.args = info.args
    this.filters = info.filters
    this.target = info.target
    this.capture = info.capture as Capture[]
    this.on = info.on as Observer['on']
    if (info.legacy) this.legacy = info.legacy
    this.filter = filter
    this.revision = revision
  }

  /** @internal Takes the engine's current view of a kept observer. */
  update(info: ObserverInfoWire, filter: CompiledFilter): void {
    this.filters = info.filters
    this.filter = filter
    this.target = info.target
    this.capture = info.capture as Capture[]
    this.on = info.on as Observer['on']
  }

  matches(doc: ObserveDoc): boolean {
    return this.filter.matches(doc)
  }

  emit(event: unknown): boolean {
    return this.owner.emit(this, event)
  }

  call<R = unknown>(payload: unknown, opts?: { timeoutMs?: number; metadata?: never }): Promise<R> {
    return this.owner.call(this, payload, opts?.timeoutMs) as Promise<R>
  }

  reject(code: string, message: string): void {
    this.owner.reject(this, code, message)
  }

  toJSON(): Record<string, unknown> {
    return {
      id: this.id,
      function_id: this.function_id,
      args: this.args,
      filters: this.filters,
      target: this.target,
      capture: this.capture,
      on: this.on,
      ...(this.legacy ? { legacy: this.legacy } : {}),
    }
  }
}

/**
 * One registered function's observers, as the engine pushed them
 * (`functionregistered`, `observeradded`, `observerremoved`). Hooks run one
 * at a time, in revision order.
 */
export class FunctionObservers {
  readonly delivery: 'auto' | 'owner'
  /** `functionregistered` arrived (cleared by a namespace conflict). */
  confirmed = false
  /** The connection (`epoch`) whose `functionregistered` set was acked: its reports may go out. */
  confirmedEpoch = -1
  /** In-process dispatch: `functionregistered.local_dispatch` ∧ the `localDispatch` option. Survives a disconnect. */
  localDispatch = false
  /** The namespace the engine registered the function in. */
  namespace?: string
  /** The highest revision received on this connection. */
  revision = -1
  /** Every acked observer (until it is removed). */
  private readonly known = new Map<string, ObserverImpl>()
  /** The active observers: one reference, swapped on change; `isObserved()` reads its length. */
  compiled: readonly ObserverImpl[] = []
  private queue: Promise<void> = Promise.resolve()
  private disposed = false
  /** Bumped on every release (conflict, unregister): an add whose hook spans one is undone. */
  private releases = 0
  /**
   * The connection (`epoch`) on which the engine refused the function
   * (`FUNCTION_NAMESPACE_CONFLICT`). On that connection a flush treats its
   * reports as gone (INACTIVE), also those of a call that was already
   * running. On a later connection the replayed registration may be accepted,
   * so its reports wait for the answer; `functionregistered` clears it.
   * (An unregistered function is gone because it left the function map.)
   */
  releasedEpoch?: number

  constructor(
    readonly functionId: string,
    readonly hooks: ObserveHooks | undefined,
    private readonly localDispatchOption: boolean,
    private readonly hasHandler: boolean,
    private readonly host: ObserveHost,
  ) {
    this.delivery = hooks?.delivery === 'owner' ? 'owner' : 'auto'
  }

  /** Callbacks or owner delivery: what needs `observe.v1` on both sides. */
  get needsObserve(): boolean {
    return this.delivery === 'owner' || !!this.hooks?.onObserverAdded || !!this.hooks?.onObserverRemoved
  }

  isObserved(): boolean {
    return this.compiled.length > 0
  }

  observers(): readonly Observer[] {
    return [...this.compiled]
  }

  /** `functionregistered`: always accepted; replaces the set (§4.2). */
  onFunctionRegistered(
    msg: { namespace: string; revision: number; local_dispatch: boolean; observers: ObserverInfoWire[] },
    epoch: number,
  ): void {
    if (this.disposed) return
    this.confirmed = true
    this.releasedEpoch = undefined
    this.localDispatch = msg.local_dispatch && this.localDispatchOption && this.hasHandler
    this.namespace = msg.namespace
    this.revision = msg.revision
    this.enqueue(async () => {
      try {
        await this.applySet(msg.observers, msg.revision, msg.namespace)
      } finally {
        // Reports held across a reconnect go out only now, after the acks: the
        // engine drops an item whose observer is still pending (INACTIVE).
        // Also when the diff failed: the engine confirmed the function.
        this.confirmedEpoch = epoch
        this.host.reporter.flush()
      }
    })
  }

  onObserverAdded(msg: { namespace: string; revision: number; observer: ObserverInfoWire }): void {
    if (this.disposed || msg.revision <= this.revision) return
    this.revision = msg.revision
    this.enqueue(async () => {
      // A fresh add of an observer this owner rejected: it starts over (removed, then added).
      if (this.known.get(msg.observer.id)?.rejection) await this.remove(msg.observer.id)
      await this.add(msg.observer, msg.revision, msg.namespace)
    })
  }

  onObserverRemoved(msg: { revision: number; id: string }): void {
    if (this.disposed || msg.revision <= this.revision) return
    this.revision = msg.revision
    this.enqueue(() => this.remove(msg.id))
  }

  /**
   * `RegistrationRejected{FUNCTION_NAMESPACE_CONFLICT}`: no local dispatch, no
   * observers, and its held reports are discarded (they could never be
   * confirmed, and would starve other functions of buffer).
   */
  onRegistrationRejected(epoch: number): void {
    this.confirmed = false
    this.confirmedEpoch = -1
    this.localDispatch = false
    this.releasedEpoch = epoch
    this.releases++
    this.host.reporter.discard(this.functionId)
    this.dropAll()
  }

  /** The function was unregistered. */
  dispose(): void {
    this.disposed = true
    this.onRegistrationRejected(-1)
  }

  /**
   * Stops delivery at once, then removes every observer through the hook
   * queue, after the adds queued before it (which undo themselves: `add`).
   */
  private dropAll(): void {
    for (const obs of this.compiled) obs.active = false
    this.compiled = []
    this.enqueue(async () => {
      for (const id of [...this.known.keys()]) await this.remove(id)
    })
  }

  private enqueue(task: () => Promise<void>): void {
    this.queue = this.queue.then(task).catch((error) => {
      this.host.logError(`Observe hook failed for ${this.functionId}`, error)
    })
  }

  private activate(obs: ObserverImpl): void {
    if (obs.active) return
    obs.active = true
    this.compiled = [...this.compiled, obs]
  }

  private deactivate(obs: ObserverImpl): void {
    if (!obs.active) return
    obs.active = false
    this.compiled = this.compiled.filter((o) => o !== obs)
  }

  private ack(info: { id: string }, revision: number, namespace: string, error?: { code: string; message: string }) {
    this.host.sendAck({ function_id: this.functionId, namespace, id: info.id, revision, ...(error ? { error } : {}) })
  }

  /** Diffs the engine's full set against what this SDK holds. */
  private async applySet(list: ObserverInfoWire[], revision: number, namespace: string): Promise<void> {
    const incoming = new Set(list.map((o) => o.id))
    for (const id of [...this.known.keys()]) {
      if (!incoming.has(id)) await this.remove(id)
    }
    for (const info of list) {
      const existing = this.known.get(info.id)
      if (existing && sameJson(existing.args, info.args) && sameJson(existing.legacy, info.legacy)) {
        this.keep(existing, info, revision, namespace)
      } else {
        if (existing) await this.remove(info.id)
        await this.add(info, revision, namespace)
      }
    }
  }

  /**
   * A kept observer: the hook already ran for it; take the engine's filters
   * and ack it again, or repeat the owner's late rejection of it.
   */
  private keep(obs: ObserverImpl, info: ObserverInfoWire, revision: number, namespace: string): void {
    if (obs.rejection) {
      obs.revision = revision
      this.ack(info, revision, namespace, obs.rejection)
      return
    }
    let filter = obs.filter
    if (!sameJson(obs.filters, info.filters)) {
      try {
        filter = compileFilter(info.filters, { trusted: true })
      } catch (error) {
        obs.revision = revision
        this.deactivate(obs)
        this.ack(info, revision, namespace, { code: 'INVALID_FILTER', message: (error as Error).message })
        return
      }
    }
    obs.update(info, filter)
    obs.revision = revision
    this.activate(obs)
    this.ack(info, revision, namespace)
  }

  /** handleAdded (§4.2): hook, then the compiled set, then the ack. */
  private async add(info: ObserverInfoWire, revision: number, namespace: string): Promise<void> {
    // Refused or unregistered since this add was queued: nothing to add, nothing to ack.
    if (this.disposed || this.releasedEpoch !== undefined) return
    const releases = this.releases
    const existing = this.known.get(info.id)
    if (existing) {
      this.keep(existing, info, revision, namespace)
      return
    }
    let filter: CompiledFilter
    try {
      filter = compileFilter(info.filters, { trusted: true })
    } catch (error) {
      this.ack(info, revision, namespace, { code: 'INVALID_FILTER', message: (error as Error).message })
      return
    }
    const obs = new ObserverImpl(this, info, filter, revision)
    const onAdded = this.hooks?.onObserverAdded
    if (onAdded) {
      obs.adding = true
      const outcome = await this.runHook(() => onAdded.call(this.hooks, obs))
      obs.adding = false
      if (!outcome.ok) {
        this.ack(info, revision, namespace, { code: outcome.code, message: outcome.message })
        if (outcome.timedOut) {
          // It may still finish and index the observer: undo that once it does.
          outcome.late.then(
            () => this.enqueue(() => this.callRemoved(obs)),
            () => undefined, // it failed after all: nothing was indexed
          )
        }
        return
      }
    }
    // Released while the hook ran: undo what the hook did; no success ack, not active.
    if (this.disposed || this.releases !== releases) {
      await this.callRemoved(obs)
      return
    }
    this.known.set(obs.id, obs)
    // Rejected (`obs.reject`) while its hook was still running: the rejection wins.
    if (obs.rejection) {
      this.ack(info, revision, namespace, obs.rejection)
      return
    }
    // In the compiled set before the ack: the first event after `active` is evaluated.
    this.activate(obs)
    this.ack(info, revision, namespace)
  }

  private async remove(id: string): Promise<void> {
    const obs = this.known.get(id)
    if (!obs) return
    this.known.delete(id)
    this.deactivate(obs)
    await this.callRemoved(obs)
  }

  private async callRemoved(obs: ObserverImpl): Promise<void> {
    const onRemoved = this.hooks?.onObserverRemoved
    if (!onRemoved) return
    const outcome = await this.runHook(() => onRemoved.call(this.hooks, obs))
    if (!outcome.ok) {
      this.host.logError(`onObserverRemoved failed for observer ${obs.id} of ${this.functionId}: ${outcome.message}`)
    }
  }

  private async runHook(fn: () => void | Promise<void>): Promise<HookOutcome> {
    const run = Promise.resolve().then(fn)
    let timer: ReturnType<typeof setTimeout> | undefined
    const timeoutMs = this.host.hookTimeoutMs()
    const timeout = new Promise<'timeout'>((resolve) => {
      timer = setTimeout(() => resolve('timeout'), timeoutMs)
      unrefTimer(timer)
    })
    try {
      const result = await Promise.race([run.then(() => 'done' as const), timeout])
      if (result === 'timeout') {
        return {
          ok: false,
          code: 'OWNER_TIMEOUT',
          message: `observe hook of ${this.functionId} still running after ${timeoutMs}ms`,
          timedOut: true,
          late: run,
        }
      }
      return { ok: true }
    } catch (error) {
      return { ok: false, ...rejectionBody(error), timedOut: false, late: run }
    } finally {
      if (timer) clearTimeout(timer)
    }
  }

  // ── owner delivery ────────────────────────────────────────────────────────

  private requireOwnerDelivery(what: string): void {
    if (this.delivery !== 'owner') {
      throw new InvocationError({
        code: 'NOT_OWNER_DELIVERED',
        message: `obs.${what}() needs an owner-delivered function; ${this.functionId} is auto-delivered (observe.delivery)`,
        function_id: this.functionId,
      })
    }
  }

  private isActive(obs: ObserverImpl): boolean {
    return obs.active && this.known.get(obs.id) === obs
  }

  /**
   * The caller an event carries (sdk.md §1): inside the function's own
   * handler, that invocation's caller; inside another handler, that handler;
   * outside any handler, this worker only.
   */
  private eventCaller(cur: ActiveInvocation | undefined): { caller?: CallerWire; own: boolean } {
    if (cur && cur.function_id === this.functionId) return { caller: cur.caller, own: true }
    const { worker_id, worker_name } = this.host.identity()
    const caller: CallerWire = worker_id !== undefined ? { worker_id, worker_name } : { worker_name }
    if (cur) {
      caller.function_id = cur.function_id
      caller.invocation_id = cur.invocation_id
      const parent = stripCaller(cur.caller)
      if (parent && Object.keys(parent).length > 0) caller.parent = parent
    }
    return { caller, own: false }
  }

  /**
   * Hands `event` to one observer. `false`, and nothing sent, when its
   * filters reject the event or it is no longer active (removed or rejected
   * meanwhile): an owner's fan-out loop must not stop for one observer.
   */
  emit(obs: ObserverImpl, event: unknown): boolean {
    this.requireOwnerDelivery('emit')
    if (!this.isActive(obs)) return false
    const cur = callContext()
    const { caller, own } = this.eventCaller(cur)
    const metadata = cur?.metadata
    if (!obs.filter.matches({ input: event, caller, metadata })) return false
    const values: { input: unknown; caller?: CallerWire; metadata?: unknown } = { input: event }
    if (caller !== undefined) values.caller = caller
    if (metadata !== undefined) values.metadata = metadata
    // The activation joins the trace the event was emitted in.
    const trace = cur ? { traceparent: cur.traceparent, baggage: cur.baggage } : this.host.traceContext()
    this.host.reporter.pushEmit({
      function_id: this.functionId,
      namespace: this.namespace,
      invocation_id: own && cur ? cur.invocation_id : randomUUID(),
      status: 'success',
      values,
      captures: obs.capture,
      started_at_ms: Date.now(),
      duration_ms: 0,
      observe_depth: cur ? cur.observe_depth : callDepth(),
      traceparent: trace.traceparent,
      baggage: trace.baggage,
      observer_id: obs.id,
      event,
      context: cur,
    })
    return true
  }

  async call(obs: ObserverImpl, payload: unknown, timeoutMs?: number): Promise<unknown> {
    this.requireOwnerDelivery('call')
    if (!this.isActive(obs)) {
      throw new InvocationError({
        code: 'OBSERVER_NOT_ACTIVE',
        message: `observer ${obs.id} of ${this.functionId} is not active`,
        function_id: this.functionId,
      })
    }
    const cur = callContext()
    const { caller } = this.eventCaller(cur)
    if (!obs.filter.matches({ input: payload, caller, metadata: cur?.metadata })) {
      throw new InvocationError({
        code: 'FILTERS_NOT_MATCHED',
        message: `the filters of observer ${obs.id} reject this payload`,
        function_id: this.functionId,
      })
    }
    return this.host.callTarget(obs.target, obs.id, payload, timeoutMs)
  }

  reject(obs: ObserverImpl, code: string, message: string): void {
    if (obs.adding) {
      // The add acks with it once the hook returns.
      obs.rejection = { code, message }
      return
    }
    if (this.known.get(obs.id) !== obs) return
    // Kept on the observer: if this ack is lost with the connection, the
    // replayed set gets it again (`keep`).
    obs.rejection = { code, message }
    this.deactivate(obs)
    this.host.sendAck({
      function_id: this.functionId,
      namespace: this.namespace,
      id: obs.id,
      revision: obs.revision,
      error: { code, message },
    })
  }

  // ── auto delivery: a local invocation ─────────────────────────────────────

  /**
   * Evaluates `set` (the compiled set captured when the call started) on a
   * finished local invocation of an auto function, and reports it when an
   * observer matches. Never throws.
   */
  reportLocal(
    set: readonly ObserverImpl[],
    init: {
      invocation_id: string
      ok: boolean
      input: unknown
      output?: unknown
      error?: ErrorBody
      caller?: CallerWire
      metadata?: unknown
      started_at_ms: number
      duration_ms: number
      observe_depth: number
      traceparent?: string
      baggage?: string
    },
  ): void {
    if (this.delivery !== 'auto' || set.length === 0) return
    const doc: ObserveDoc = { input: init.input, caller: init.caller, metadata: init.metadata }
    if (init.ok) doc.output = init.output
    else doc.error = init.error
    const matched: string[] = []
    const captures = new Set<string>()
    for (const obs of set) {
      if (!onAccepts(obs.on, init.ok)) continue
      if (!obs.filter.matches(doc)) continue
      matched.push(obs.id)
      for (const c of obs.capture) captures.add(c)
    }
    if (matched.length === 0) return
    // Pushed even if the function was released meanwhile: the flush decides
    // against the registration current then (`gone` → INACTIVE, re-confirmed → sent).
    const values: Record<string, unknown> = { input: init.input }
    // The error as the routed path reports it: the handler's error body.
    if (init.ok) values.output = init.output
    else values.error = init.error
    if (init.caller !== undefined) values.caller = init.caller
    if (init.metadata !== undefined) values.metadata = init.metadata
    this.host.reporter.push({
      function_id: this.functionId,
      namespace: this.namespace,
      invocation_id: init.invocation_id,
      matched,
      status: init.ok ? 'success' : 'error',
      values,
      captures,
      started_at_ms: init.started_at_ms,
      duration_ms: init.duration_ms,
      observe_depth: init.observe_depth,
      traceparent: init.traceparent,
      baggage: init.baggage,
    })
  }
}
