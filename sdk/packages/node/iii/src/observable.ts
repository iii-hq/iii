/**
 * The host side of observe (v2 §4): per-function observer state, the
 * observer hooks of the worker and of the function, the `Observer` object
 * handed to them, keyed delivery, and the delivery of each call of a hosted
 * function.
 */
import { InvocationError, ObserveRejection } from './errors'
import { type CompiledFilter, compileFilter, type FilterDoc } from './filter'
import type { CallerWire, ErrorBody, ObserverInfoWire } from './iii-types'
import type {
  AddedHook,
  Call,
  Capture,
  DeliverHook,
  Filter,
  KeyedSpec,
  ObserveReason,
  Observer,
  RemovedHook,
} from './observe-types'
import type { Reporter, ReportInit, ReportValues } from './reporter'

/** CONTRACT D11: an observer hook still running after this is `OWNER_TIMEOUT`. */
export const OBSERVE_HOOK_TIMEOUT_MS = 30_000

/** The hooks that apply to one function: the worker's, and its own. */
export type FunctionHooks = {
  workerAdded?: AddedHook
  workerRemoved?: RemovedHook
  added?: AddedHook
  removed?: RemovedHook
  deliver?: DeliverHook
}

export interface ObserveHost {
  sendAck(ack: {
    function_id: string
    namespace?: string
    id: string
    revision: number
    error?: { code: string; message: string }
  }): void
  readonly reporter: Reporter
  /** The hooks set for `functionId` now: they are read when they run, so a hook set later applies. */
  hooks(functionId: string): FunctionHooks
  logError(message: string, error?: unknown): void
  logWarn(message: string): void
  hookTimeoutMs(): number
  /** The engine advertised `observe.v2`: report items may carry `output_is_input`. */
  observeV2(): boolean
}

/** A hook level: the worker's hooks (every function it hosts) or the function's own. */
type Level = 'worker' | 'fn'

/**
 * Where an observer stands at one level: `none` (not set up there, or torn
 * down), `bare` (set up while the level had no `onObserverAdded`), `hooked`
 * (its `onObserverAdded` resolved). A level that is set up is torn down
 * exactly once, by its `onObserverRemoved`.
 */
type LevelState = 'none' | 'bare' | 'hooked'

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

/** The error a handler's (or a deliver hook's) failure becomes, the same on the local and the remote path. */
export function handlerErrorBody(error: unknown): ErrorBody {
  const isError = error instanceof Error
  return {
    code: 'invocation_failed',
    message: isError ? error.message : String(error),
    stacktrace: isError ? error.stack : undefined,
  }
}

/** What filters read from a call; a call without `status` reads as a successful one. */
function filterDoc(call: Partial<Call>): FilterDoc {
  const doc: FilterDoc = { input: call.input, caller: call.caller, metadata: call.metadata }
  if (call.status === 'error') doc.error = call.error
  else doc.output = call.output
  return doc
}

/**
 * The caller a report item carries: the engine sets `worker_id` and
 * `worker_name` itself, so they are left out (a `parent` keeps its own).
 * Nothing for a call made outside any handler.
 */
function reportCaller(caller: CallerWire | undefined): CallerWire | undefined {
  if (!caller) return undefined
  const { worker_id: _, worker_name: __, ...rest } = caller
  return Object.keys(rest).length > 0 ? rest : undefined
}

/** The same call (decision 36): every field holds the same value, compared by identity. */
function sameCall(a: Call, b: Call): boolean {
  return (
    a === b ||
    (a.input === b.input &&
      a.status === b.status &&
      a.output === b.output &&
      a.error === b.error &&
      a.caller === b.caller &&
      a.metadata === b.metadata)
  )
}

const KEYED = Symbol('iii.keyed')

/** The keys behind a deliver hook made by {@link keyed}, if it is one. */
function keyedSpecOf(hook: DeliverHook | undefined): KeyedSpec | undefined {
  return (hook as { [KEYED]?: KeyedSpec } | undefined)?.[KEYED]
}

function keyList(keys: string | readonly string[]): readonly string[] {
  return typeof keys === 'string' ? [keys] : keys
}

/**
 * Keyed delivery (v2 §4.5): set as a function's `onObserverDeliver`, the SDK
 * files each observer under the keys `spec.observer` returns, once, when it is
 * accepted, and unfiles it when it is removed. A call reaches the observers
 * filed under any of the keys `spec.call` returns, each at most once, through
 * `next`, so `on`, filters, policy and `deliver` apply. A call costs one hash
 * lookup per key, whatever the number of observers.
 *
 * Keys express wildcards by listing them: an observer of every table is filed
 * under `primary|*|insert`, and a change is delivered to
 * `primary|orders|insert` and `primary|*|insert`.
 *
 * @example
 * ```typescript
 * iii.onObserverDeliver('cron::on-tick', keyed({
 *   observer: (obs) => obs.args.expression as string,
 *   call: (call) => call.input.expression,
 * }))
 * ```
 */
// biome-ignore lint/suspicious/noExplicitAny: the call's types default to any so untyped key functions read their input
export function keyed<I = any, O = any>(spec: KeyedSpec<I, O>): DeliverHook<I, O, O> {
  // Called as a plain function (wrapped in another hook), it scans the observers it is given.
  const hook: DeliverHook<I, O, O> = (call, observers, next) => {
    const wanted = new Set(keyList(spec.call(call)))
    return next(
      call,
      observers.filter((obs) => keyList(spec.observer(obs)).some((key) => wanted.has(key))),
    )
  }
  return Object.assign(hook, { [KEYED]: spec as KeyedSpec })
}

/** The `Observer` a host receives. Its identity is stable while the observer lives. */
export class ObserverImpl implements Observer {
  readonly id: string
  readonly function_id: string
  args: Record<string, unknown>
  filters: Filter
  target: string
  on: 'success' | 'error' | 'complete'
  capture: Capture[]
  /** @internal */
  filter: CompiledFilter
  /** @internal The revision it was acked with (echoed by a late rejection). */
  revision: number
  /** @internal In the active set: delivery reaches it. */
  active = false
  /**
   * @internal The host's late rejection (`obs.reject`). It survives a
   * reconnect: a replayed set that lists the observer is acked with it again,
   * and only a fresh `observeradded` reactivates the observer.
   */
  rejection?: ObserveReason
  /** @internal Its `onObserverAdded` hooks are running: a `reject()` now is applied by the add, not sent. */
  adding = false
  /** @internal Where it stands at each hook level. */
  readonly levels: Record<Level, LevelState> = { worker: 'none', fn: 'none' }
  /** @internal The keys it is filed under (keyed delivery), computed once. */
  keys?: readonly string[]
  private acceptance: 'pending' | 'accepted' | ObserveReason = 'pending'
  private waiters?: { resolve: () => void; reject: (error: ObserveRejection) => void }[]

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
    this.on = info.on as Observer['on']
    this.capture = info.capture as Capture[]
    this.filter = filter
    this.revision = revision
  }

  /** @internal Takes the engine's current view of a kept observer. */
  update(info: ObserverInfoWire, filter: CompiledFilter): void {
    this.filters = info.filters
    this.filter = filter
    this.target = info.target
    this.on = info.on as Observer['on']
    this.capture = info.capture as Capture[]
  }

  matches(call: Partial<Call>): boolean {
    return onAccepts(this.on, call.status !== 'error') && this.filter.matches(filterDoc(call))
  }

  accepted(): Promise<void> {
    const state = this.acceptance
    if (state === 'accepted') return Promise.resolve()
    if (state !== 'pending') return Promise.reject(new ObserveRejection(state.code, state.message))
    return new Promise<void>((resolve, reject) => {
      this.waiters ??= []
      this.waiters.push({ resolve, reject })
    })
  }

  reject(code: string, message: string): void {
    this.owner.reject(this, code, message)
  }

  /** @internal Acked (`accepted`) or never to be: settles `accepted()`, once. */
  settleAcceptance(outcome: 'accepted' | ObserveReason): void {
    if (this.acceptance !== 'pending') return
    this.acceptance = outcome
    const waiters = this.waiters ?? []
    this.waiters = undefined
    for (const waiter of waiters) {
      if (outcome === 'accepted') waiter.resolve()
      else waiter.reject(new ObserveRejection(outcome.code, outcome.message))
    }
  }

  toJSON(): Record<string, unknown> {
    return {
      id: this.id,
      function_id: this.function_id,
      args: this.args,
      filters: this.filters,
      target: this.target,
      on: this.on,
      capture: this.capture,
    }
  }
}

/**
 * One registered function's observers, as the engine pushed them
 * (`functionregistered`, `observeradded`, `observerremoved`). Hooks run one
 * at a time, in the order the engine announced the observers.
 */
export class FunctionObservers {
  /** `functionregistered` arrived (cleared by a refusal). */
  confirmed = false
  /** The connection (`epoch`) whose `functionregistered` set was acked: its reports may go out. */
  confirmedEpoch = -1
  /** In-process dispatch: `functionregistered.local_dispatch` ∧ the `localDispatch` option. Survives a disconnect. */
  localDispatch = false
  /** The namespace the engine registered the function in. */
  namespace?: string
  /** The highest revision received on this connection. */
  revision = -1
  /** Every observer the hooks set up, until it is removed. */
  private readonly known = new Map<string, ObserverImpl>()
  /** The active observers: one reference, swapped on change; `isObserved()` reads its length. */
  compiled: readonly ObserverImpl[] = []
  private queue: Promise<void> = Promise.resolve()
  private disposed = false
  /** Bumped on every release (conflict, unregister): an add whose hooks span one is undone. */
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
  /** The keys the index is built for: the function's deliver hook, when {@link keyed} made it. */
  private keyedSpec?: KeyedSpec
  /** Keyed delivery: the active observers by key. */
  private index?: Map<string, Set<ObserverImpl>>

  constructor(
    readonly functionId: string,
    private readonly localDispatchOption: boolean,
    private readonly hasHandler: boolean,
    private readonly host: ObserveHost,
  ) {
    this.deliverChanged()
  }

  get reporter(): Reporter {
    return this.host.reporter
  }

  /** @internal */
  observeV2(): boolean {
    return this.host.observeV2()
  }

  /**
   * Whether a call would reach anyone. Without a call: any active observer
   * (one load). With a call, under keyed delivery: an observer filed under one
   * of its keys (one lookup per key). Otherwise the call is not read.
   */
  isObserved(call?: Partial<Call>): boolean {
    if (this.compiled.length === 0) return false
    const spec = this.keyedSpec
    const index = this.index
    if (call === undefined || !spec || !index) return true
    for (const key of keyList(spec.call(call as Call))) {
      if (index.has(key)) return true
    }
    return false
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
      // A fresh add of an observer this host rejected: it starts over (removed, then added).
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
   * The engine refused the function (`registrationrejected`), or it no
   * longer holds it here: no local dispatch, no observers, and its held
   * reports are discarded (they could never be confirmed, and would starve
   * other functions of buffer).
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
   * An `onObserverAdded` was set at `level` (the worker's, or this
   * function's): the active observers that were set up without one there are
   * caught up. It runs for each of them, in the hook queue; a throw
   * late-rejects that observer. One warning names the function and how many
   * observers were active without the hook.
   */
  catchUp(level: Level): void {
    if (this.disposed) return
    this.enqueue(async () => {
      const behind = this.compiled.filter((obs) => obs.levels[level] === 'bare')
      if (behind.length === 0) return
      this.host.logWarn(
        `onObserverAdded was set for ${this.functionId} while ${behind.length} observer(s) were active without it; it runs for each of them now`,
      )
      for (const obs of behind) {
        const hook = this.addedHook(level)
        if (!hook) return
        if (!obs.active || obs.levels[level] !== 'bare') continue
        const outcome = await this.runHook(() => hook(obs))
        if (outcome.ok) {
          obs.levels[level] = 'hooked'
          continue
        }
        // Not set up at this level: its teardown is for the other one.
        obs.levels[level] = 'none'
        if (outcome.timedOut) this.undoLate(outcome.late, obs, level)
        this.reject(obs, outcome.code, outcome.message)
      }
    })
  }

  /**
   * The function's deliver hook was set or removed. Keyed delivery files the
   * active observers anew under the keys of the new hook; an observer whose
   * keys cannot be computed is late-rejected.
   */
  deliverChanged(): void {
    const spec = keyedSpecOf(this.host.hooks(this.functionId).deliver)
    if (spec === this.keyedSpec) return
    this.keyedSpec = spec
    this.index = spec ? new Map() : undefined
    for (const obs of this.known.values()) obs.keys = undefined
    for (const obs of this.compiled) {
      const failure = this.prepareKeys(obs)
      if (failure) this.reject(obs, failure.code, failure.message)
      else this.file(obs)
    }
  }

  /** @internal The index is built for `spec`: a call is delivered from it. */
  indexes(spec: KeyedSpec): boolean {
    return this.keyedSpec === spec && this.index !== undefined
  }

  /** @internal The observers filed under the call's keys, each once. */
  keyedObservers(call: Call): ObserverImpl[] {
    const spec = this.keyedSpec
    const index = this.index
    if (!spec || !index) return []
    const found = new Set<ObserverImpl>()
    for (const key of keyList(spec.call(call))) {
      const filed = index.get(key)
      if (filed) for (const obs of filed) found.add(obs)
    }
    return [...found]
  }

  /**
   * Stops delivery at once, then removes every observer through the hook
   * queue, after the adds queued before it (which undo themselves: `add`).
   */
  private dropAll(): void {
    for (const obs of this.compiled) this.deactivate(obs)
    this.enqueue(async () => {
      for (const id of [...this.known.keys()]) await this.remove(id)
    })
  }

  private enqueue(task: () => Promise<void>): void {
    this.queue = this.queue.then(task).catch((error) => {
      this.host.logError(`Observer hook failed for ${this.functionId}`, error)
    })
  }

  private addedHook(level: Level): AddedHook | undefined {
    const hooks = this.host.hooks(this.functionId)
    return level === 'worker' ? hooks.workerAdded : hooks.added
  }

  private removedHook(level: Level): RemovedHook | undefined {
    const hooks = this.host.hooks(this.functionId)
    return level === 'worker' ? hooks.workerRemoved : hooks.removed
  }

  private activate(obs: ObserverImpl): void {
    if (obs.active) return
    obs.active = true
    this.compiled = [...this.compiled, obs]
    this.file(obs)
  }

  private deactivate(obs: ObserverImpl): void {
    if (!obs.active) return
    obs.active = false
    this.compiled = this.compiled.filter((o) => o !== obs)
    this.unfile(obs)
  }

  /** Computes the keys `obs` is filed under, once, when the function's deliver hook is keyed. */
  private prepareKeys(obs: ObserverImpl): ObserveReason | undefined {
    const spec = this.keyedSpec
    if (!spec || obs.keys) return undefined
    try {
      obs.keys = [...new Set(keyList(spec.observer(obs)))]
      return undefined
    } catch (error) {
      return {
        code: 'OBSERVE_REJECTED',
        message: `the keys of observer ${obs.id} of ${this.functionId} could not be computed: ${error instanceof Error ? error.message : String(error)}`,
      }
    }
  }

  private file(obs: ObserverImpl): void {
    const index = this.index
    if (!index || !obs.keys) return
    for (const key of obs.keys) {
      const filed = index.get(key)
      if (filed) filed.add(obs)
      else index.set(key, new Set([obs]))
    }
  }

  private unfile(obs: ObserverImpl): void {
    const index = this.index
    if (!index || !obs.keys) return
    for (const key of obs.keys) {
      const filed = index.get(key)
      if (!filed) continue
      filed.delete(obs)
      if (filed.size === 0) index.delete(key)
    }
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
      if (existing && sameJson(existing.args, info.args)) {
        await this.keep(existing, info, revision, namespace)
      } else {
        if (existing) await this.remove(info.id)
        await this.add(info, revision, namespace)
      }
    }
  }

  /**
   * A kept observer: its hooks already ran; take the engine's filters and
   * ack it again, or repeat the host's late rejection of it.
   */
  private async keep(obs: ObserverImpl, info: ObserverInfoWire, revision: number, namespace: string): Promise<void> {
    obs.revision = revision
    if (obs.rejection) {
      this.ack(info, revision, namespace, obs.rejection)
      return
    }
    let filter = obs.filter
    if (!sameJson(obs.filters, info.filters)) {
      try {
        filter = compileFilter(info.filters, { trusted: true })
      } catch (error) {
        this.deactivate(obs)
        this.ack(info, revision, namespace, { code: 'INVALID_FILTER', message: (error as Error).message })
        return
      }
    }
    obs.update(info, filter)
    const failure = this.prepareKeys(obs)
    if (failure) {
      obs.rejection = failure
      this.deactivate(obs)
      this.ack(info, revision, namespace, failure)
      await this.tearDown(obs)
      return
    }
    this.activate(obs)
    this.ack(info, revision, namespace)
  }

  /** handleAdded (§4.3): the hooks of both levels, then the active set, then the ack. */
  private async add(info: ObserverInfoWire, revision: number, namespace: string): Promise<void> {
    // Refused or unregistered since this add was queued: nothing to add, nothing to ack.
    if (this.disposed || this.releasedEpoch !== undefined) return
    const releases = this.releases
    const existing = this.known.get(info.id)
    if (existing) {
      await this.keep(existing, info, revision, namespace)
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
    obs.adding = true
    const failure = await this.setUp(obs)
    obs.adding = false
    if (failure) {
      this.ack(info, revision, namespace, failure)
      obs.settleAcceptance(failure)
      return
    }
    // Released while the hooks ran: undo what they did; no success ack, not active.
    if (this.disposed || this.releases !== releases) {
      await this.tearDown(obs)
      obs.settleAcceptance({
        code: 'OBSERVER_NOT_ACTIVE',
        message: `${this.functionId} was released before observer ${obs.id} was accepted`,
      })
      return
    }
    this.known.set(obs.id, obs)
    // Rejected (`obs.reject`) while its hooks were still running: the rejection wins.
    const rejection = obs.rejection ?? this.prepareKeys(obs)
    if (rejection) {
      obs.rejection = rejection
      this.ack(info, revision, namespace, rejection)
      obs.settleAcceptance(rejection)
      await this.tearDown(obs)
      return
    }
    // In the active set before the ack: the first call after `active` is delivered to it.
    this.activate(obs)
    this.ack(info, revision, namespace)
    obs.settleAcceptance('accepted')
  }

  /**
   * Runs `onObserverAdded` of the worker, then of the function. When one
   * fails, the levels already set up are torn down, and the rejection is
   * returned.
   */
  private async setUp(obs: ObserverImpl): Promise<ObserveReason | undefined> {
    for (const level of ['worker', 'fn'] as const) {
      const hook = this.addedHook(level)
      if (!hook) {
        obs.levels[level] = 'bare'
        continue
      }
      const outcome = await this.runHook(() => hook(obs))
      if (!outcome.ok) {
        if (outcome.timedOut) this.undoLate(outcome.late, obs, level)
        await this.tearDown(obs)
        return { code: outcome.code, message: outcome.message }
      }
      obs.levels[level] = 'hooked'
    }
    return undefined
  }

  /** Runs `onObserverRemoved` of the function, then of the worker, for each level set up, once. */
  private async tearDown(obs: ObserverImpl): Promise<void> {
    for (const level of ['fn', 'worker'] as const) {
      if (obs.levels[level] === 'none') continue
      obs.levels[level] = 'none'
      await this.runRemoved(obs, level)
    }
  }

  /** A timed-out `onObserverAdded` that resolves later is undone by its level's `onObserverRemoved`. */
  private undoLate(late: Promise<unknown>, obs: ObserverImpl, level: Level): void {
    late.then(
      () => this.enqueue(() => this.runRemoved(obs, level)),
      () => undefined, // it failed after all: nothing was set up
    )
  }

  private async remove(id: string): Promise<void> {
    const obs = this.known.get(id)
    if (!obs) return
    this.known.delete(id)
    this.deactivate(obs)
    await this.tearDown(obs)
  }

  private async runRemoved(obs: ObserverImpl, level: Level): Promise<void> {
    const hook = this.removedHook(level)
    if (!hook) return
    const outcome = await this.runHook(() => hook(obs))
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
      timer.unref?.()
    })
    try {
      const result = await Promise.race([run.then(() => 'done' as const), timeout])
      if (result === 'timeout') {
        return {
          ok: false,
          code: 'OWNER_TIMEOUT',
          message: `observer hook of ${this.functionId} still running after ${timeoutMs}ms`,
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

  /** @internal Late rejection (`obs.reject`, a failed catch-up): inactive at once, then torn down. */
  reject(obs: ObserverImpl, code: string, message: string): void {
    if (obs.adding) {
      // The add acks with it once the hooks return.
      obs.rejection = { code, message }
      return
    }
    if (this.known.get(obs.id) !== obs || obs.rejection) return
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
    this.enqueue(() => this.tearDown(obs))
  }
}

/** What one call of a hosted function is delivered from (v2 §4.4). */
export type DeliveryInit = {
  /** The handler's call: its input and outcome. */
  call: Call
  /** What the handler threw, when `call.status` is `error`. */
  thrown?: unknown
  /** The function's active observers when the call started. */
  snapshot: readonly ObserverImpl[]
  started_at_ms: number
  /** How long the handler ran. */
  duration_ms: number
  observe_depth: number
  traceparent?: string
  baggage?: string
  /**
   * A routed call: its items are handed here once the hook settles, to go
   * out ahead of the answer, and the answer lists {@link Delivery.observed}.
   * A local call's items go to the reporter.
   */
  routed?: (items: ReportInit[]) => void
}

/**
 * The delivery of one call (v2 §4.4, decisions 20, 36): runs the function's
 * `onObserverDeliver` with `next`, or `next` alone, and reports what each
 * `next` delivered. Each observer is delivered at most once per call. The
 * `next` calls given the handler's call unchanged pool what they deliver: in
 * the routed answer's `observed` when that answer is the handler's own
 * outcome, otherwise in one item that carries the handler's call. Each `next`
 * call given a changed call reports its own item.
 */
export class Delivery {
  private readonly delivered = new Set<string>()
  /** Delivered by the `next` calls given the handler's call unchanged. */
  private readonly pooled: ObserverImpl[] = []
  /** A routed call's items, until the hook settles. */
  private readonly items: ReportInit[] = []
  private settled = false
  /** A routed call's `observed`: the pooled ids, when the answer is the handler's own outcome. */
  observed: readonly string[] = []

  constructor(
    private readonly owner: FunctionObservers,
    private readonly init: DeliveryInit,
  ) {}

  /** iii's delivery, as the deliver hook's `next`. */
  readonly next = (call: Call, observers: readonly Observer[]): Promise<unknown> => {
    this.deliver(call, observers)
    return this.outcome(call)
  }

  /**
   * Runs `hook` (or iii's delivery without one) and settles the call's
   * reports: resolves to what the caller gets back, or rejects with what the
   * hook threw.
   */
  async run(hook: DeliverHook | undefined): Promise<unknown> {
    const { call, snapshot, thrown } = this.init
    let value: unknown
    try {
      const spec = keyedSpecOf(hook)
      if (hook === undefined) value = await this.next(call, snapshot)
      else if (spec && this.owner.indexes(spec)) value = await this.next(call, this.owner.keyedObservers(call))
      else value = await hook(call, snapshot, this.next)
    } catch (error) {
      this.settle(call.status === 'error' && error === thrown)
      throw error
    }
    this.settle(call.status === 'success' && value === call.output)
    return value
  }

  /** The error body an answer carries for `error`: the handler's own, or what the hook threw. */
  errorBody(error: unknown): ErrorBody {
    const { call, thrown } = this.init
    return error === thrown && call.error ? call.error : handlerErrorBody(error)
  }

  private deliver(call: Call, observers: readonly Observer[]): void {
    if (this.settled && this.init.routed) {
      // The answer is out: the engine would refuse the item (INVALID_REPORT).
      this.owner.reporter.drop(this.owner.functionId, 'LATE_DELIVERY')
      return
    }
    const matched = this.match(call, observers)
    if (matched.length === 0) return
    if (this.settled || !sameCall(call, this.init.call)) this.report(this.item(call, matched))
    else this.pooled.push(...matched)
  }

  /** The observers given whose `on` and filters match `call`, less those already delivered. */
  private match(call: Call, observers: readonly Observer[]): ObserverImpl[] {
    const ok = call.status !== 'error'
    const doc = filterDoc(call)
    const matched: ObserverImpl[] = []
    for (const obs of observers) {
      if (!(obs instanceof ObserverImpl) || obs.function_id !== this.owner.functionId) continue
      if (this.delivered.has(obs.id) || !onAccepts(obs.on, ok) || !obs.filter.matches(doc)) continue
      this.delivered.add(obs.id)
      matched.push(obs)
    }
    return matched
  }

  /** What `next` resolves to: the call's output, or its error. */
  private outcome(call: Call): Promise<unknown> {
    if (call.status !== 'error') return Promise.resolve(call.output)
    const { call: handlerCall, thrown } = this.init
    const error =
      call.error === handlerCall.error
        ? thrown
        : new InvocationError({
            code: call.error?.code ?? 'invocation_failed',
            message: call.error?.message ?? '',
            function_id: this.owner.functionId,
          })
    const rejected = Promise.reject(error)
    // Delivered either way: a hook that does not await `next` leaves no unhandled rejection.
    rejected.catch(() => undefined)
    return rejected
  }

  private report(item: ReportInit): void {
    if (this.init.routed) this.items.push(item)
    else this.owner.reporter.push(item)
  }

  private settle(own: boolean): void {
    if (this.settled) return
    this.settled = true
    const { routed, call } = this.init
    if (this.pooled.length > 0) {
      if (routed && own) this.observed = this.pooled.map((obs) => obs.id)
      else this.report(this.item(call, this.pooled))
    }
    if (routed && this.items.length > 0) routed(this.items.splice(0))
  }

  /** A report item: `call` as delivered to `matched`, under the handler call's invocation. */
  private item(call: Call, matched: readonly ObserverImpl[]): ReportInit {
    const init = this.init
    const captures = new Set<string>()
    for (const obs of matched) for (const c of obs.capture) captures.add(c)
    const ok = call.status !== 'error'
    const values: ReportValues = { input: call.input }
    if (ok) values.output = call.output
    else values.error = call.error
    const caller = reportCaller(call.caller)
    if (caller !== undefined) values.caller = caller
    if (call.metadata !== undefined) values.metadata = call.metadata
    return {
      function_id: this.owner.functionId,
      namespace: this.owner.namespace,
      invocation_id: init.call.invocation_id,
      matched: matched.map((obs) => obs.id),
      status: ok ? 'success' : 'error',
      values,
      captures,
      output_is_input: ok && call.output === call.input && this.owner.observeV2(),
      started_at_ms: init.started_at_ms,
      duration_ms: init.duration_ms,
      observe_depth: init.observe_depth,
      traceparent: init.traceparent,
      baggage: init.baggage,
    }
  }
}
