import type { RegisterFunctionFormat } from './iii-types'

/** What an observer ships to its target. Default `['input', 'output', 'caller']`. */
export type Capture = 'input' | 'output' | 'error' | 'caller' | 'metadata'

/**
 * An observer's lifecycle: `pending` (no owner yet, waiting for the owner's
 * ack, or no target yet) → `active` or `error`; `suspended` while its worker
 * or target is away; `disposed` at the end.
 */
export type ObserverState = 'pending' | 'active' | 'suspended' | 'error' | 'disposed'

/**
 * A declarative filter over `input`, `output`, `error`, `caller` and
 * `metadata`, with explicit operators only:
 * `{ 'input.amount': { $gte: 500 } }`. See the filter language reference.
 */
export type Filter = Record<string, unknown>

/** The right side of `iii.observe`: a registered function. */
export type Target = string | { function_id: string; namespace?: string }

/** A state reason or an error: a machine-readable code and a message. */
export type ObserveReason = { code: string; message: string }

/**
 * Who started an invocation: the worker (`worker_id` / `worker_name`, stamped
 * by the engine), and the function and invocation the call was made from.
 */
export interface Caller {
  /** Absent on activations, whose caller is the engine (`worker_name: 'iii-engine'`). */
  worker_id?: string
  worker_name?: string
  /** The handler the call was made from; absent for top-level calls. */
  function_id?: string
  invocation_id?: string
  /** The caller of that handler, one level only. */
  parent?: Omit<Caller, 'parent'>
}

/**
 * A caller as the SDK knows it: every field may be missing (a top-level call
 * has no function; a call made before `WorkerRegistered` has no worker id).
 */
export type CallerInfo = Partial<Omit<Caller, 'parent'>> & { parent?: Partial<Omit<Caller, 'parent'>> }

/**
 * What the target of an auto observer (or of `obs.emit`) receives with
 * `deliver: 'envelope'` (the default). Fields not in the observer's
 * `capture` are absent.
 */
export interface Observed<I = unknown, O = unknown> {
  observer_id: string
  activation_id: string
  function_id: string
  namespace: string
  invocation_id: string
  status: 'success' | 'error'
  input?: I
  output?: O
  error?: { code: string; message: string }
  caller?: Caller
  metadata?: Record<string, unknown>
  started_at_ms: number
  duration_ms: number
}

/** The left side of `iii.observe`: what is observed. */
export interface ObserveSource {
  /** The observed function. */
  function_id: string
  /** Defaults to this worker's namespace (`engine::*` resolves to `default`). */
  namespace?: string
  /** Owner-delivered functions only: for the owner; validated against its `args_format`. */
  args?: Record<string, unknown>
  /** Which events reach the target; `{}` (or absent) = every event. */
  filters?: Filter
  /** Auto only; default `'success'`. */
  on?: 'success' | 'error' | 'complete'
  /** Auto and `obs.emit`; default `['input', 'output', 'caller']`. */
  capture?: Capture[]
  /** Auto and `obs.emit`; default `'envelope'`. */
  deliver?: 'envelope' | 'input' | 'output'
}

/** The observer itself: its id, policy and metadata. */
export interface ObserverOptions {
  /** Default: a random UUID, stable across replays. */
  id?: string
  /** The observer is disposed after this many activations (`MAX_FIRES`). Default: unlimited. */
  max_fires?: number
  /** The observer is disposed this many milliseconds after it is created (`EXPIRED`). Default: never. */
  expires_in_ms?: number
  /**
   * Activations running at once; further ones queue, then drop as `OVERFLOW`.
   * Default: the engine's `observers.default_max_in_flight` (64).
   */
  max_in_flight?: number
  /** Delivered as the metadata sidecar of every activation. */
  metadata?: Record<string, unknown>
}

/** What `iii.observe` returns, synchronously. */
export interface ObserverHandle {
  readonly id: string
  readonly state: ObserverState
  /** Why the observer is in its state (`error`, `suspended`, `disposed`). */
  readonly reason?: ObserveReason
  /**
   * Resolves when the observer is `active`; rejects with an
   * {@link ObserveRejection} carrying the code when it turns `error` or
   * `disposed` first.
   */
  ready(): Promise<void>
  /**
   * Calls `listener` on every state change, in order; returns the
   * unsubscribe function. Listeners are called asynchronously (never on the
   * connection's read path), so they may call the client.
   */
  onStateChange(listener: (state: ObserverState, reason?: ObserveReason) => void): () => void
  /** Removes the observer (`unobserve`). It is not replayed on reconnect. */
  dispose(): Promise<void>
}

/** The document an observer's filters read (filters.md §1). */
export type ObserveDoc = {
  input?: unknown
  output?: unknown
  error?: unknown
  caller?: CallerInfo
  metadata?: unknown
}

/**
 * An observer as the function's owner sees it (`onObserverAdded`,
 * `FunctionRef.observers()`): its args, filters, target, capture and `on`.
 * Never its metadata, policy or the observing worker.
 */
export interface Observer {
  readonly id: string
  /** The observed function (the one this worker hosts). */
  readonly function_id: string
  /** The observer's args, validated against `args_format` by the engine. */
  readonly args: Record<string, unknown>
  /** The observer's filters, normalized; the SDK applies them in `emit` / `call`. */
  readonly filters: Filter
  /** The target function id. */
  readonly target: string
  readonly capture: Capture[]
  /** Auto only. */
  readonly on: 'success' | 'error' | 'complete'
  /** Set when the engine's legacy shim created the observer from an old `registerTrigger`. */
  readonly legacy?: { trigger_type: string; config: unknown }
  /** Evaluates the observer's filters on a document. */
  matches(doc: ObserveDoc): boolean
  /**
   * Owner delivery, fire-and-forget: hands `event` to this observer. Returns
   * `false` and sends nothing when its filters reject the event or the
   * observer is no longer active (removed or rejected meanwhile). Throws
   * `NOT_OWNER_DELIVERED` on an auto function.
   */
  emit(event: unknown): boolean
  /**
   * Owner delivery, request/response: invokes the target with `payload` and
   * resolves with its result. Rejects with `FILTERS_NOT_MATCHED` (nothing is
   * sent) when the filters reject the payload, and with `OBSERVER_NOT_ACTIVE`
   * when the observer is no longer active.
   */
  call<R = unknown>(payload: unknown, opts?: { timeoutMs?: number; metadata?: never }): Promise<R>
  /**
   * Late rejection of an active observer (e.g. after a config reload). It
   * stops receiving events at once, and stays rejected across reconnects
   * until the engine adds it again.
   */
  reject(code: string, message: string): void
}

/**
 * The `observe` option of `registerFunction`: how the function's observers
 * are delivered, and the callbacks that run when one is added or removed.
 *
 * The callbacks of one function run one at a time, in revision order, with
 * one exception: a callback still running after 30 s is treated as failed
 * (`OWNER_TIMEOUT`) and the next one starts, so a hung callback cannot stall
 * its function forever. It may then overlap with the callbacks after it, and
 * if a timed-out `onObserverAdded` completes later, `onObserverRemoved` runs
 * for that observer.
 */
export interface ObserveHooks {
  /**
   * `auto` (default): every invocation is offered to every observer.
   * `owner`: the handler hands each event to the observers it belongs to,
   * with `obs.emit` / `obs.call`.
   */
  delivery?: 'auto' | 'owner'
  /** Owner only: JSON Schema of the observers' `args` (the engine answers `INVALID_ARGS`). */
  args_format?: RegisterFunctionFormat
  /** Accept old `registerTrigger({ type })` calls for this trigger type. */
  legacy_trigger_type?: string
  /** Default `true`; `false` refuses every observer (`NOT_OBSERVABLE`). */
  allow?: boolean
  /**
   * Runs once per added observer, before the SDK acks it. Throw to reject it
   * (an {@link ObserveRejection} sets the code, anything else is
   * `OBSERVE_REJECTED`); after 30 s it is rejected with `OWNER_TIMEOUT`.
   */
  onObserverAdded?(obs: Observer): void | Promise<void>
  /** Runs once per removed observer. Cannot fail: errors are logged. */
  onObserverRemoved?(obs: Observer): void | Promise<void>
}

/**
 * The invocation the current code runs in, in every handler, remote or
 * local (`iii.currentInvocation()`).
 */
export interface InvocationContext {
  function_id: string
  invocation_id: string
  caller?: CallerInfo
  /** Set when this invocation is the activation of an observer's target. */
  activation?: { observer_id: string; activation_id?: string }
  observe_depth: number
}
