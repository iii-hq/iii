/** What an observer ships to its target. Default `['input', 'output', 'caller']`. */
export type Capture = 'input' | 'output' | 'error' | 'caller' | 'metadata'

/**
 * An observer's lifecycle: `pending` (no host yet, waiting for the host's
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
 * What an observer's target receives when iii delivers with
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
  /**
   * For the worker that hosts the function: what to set up and which calls
   * to deliver (a table, a cron expression, a route). The engine validates
   * them against the function's `args_format`.
   */
  args?: Record<string, unknown>
  /** Which calls reach the target, by `input`, `output`, `error`, `caller` and `metadata`; `{}` (or absent) = every call. */
  filters?: Filter
  /** Which outcomes reach the target; default `'success'`. */
  on?: 'success' | 'error' | 'complete'
  /** What the envelope carries; default `['input', 'output', 'caller']`. */
  capture?: Capture[]
  /**
   * What the target receives when iii delivers: the {@link Observed} envelope
   * (default), the call's `input`, or its `output`.
   */
  deliver?: 'envelope' | 'input' | 'output'
}

/** The observer itself: its id, policy and metadata. */
export interface ObserverOptions {
  /**
   * Default: a random UUID, stable across replays. The handle holds it until
   * it is disposed: an `observe` of an id a handle of this client holds
   * throws an {@link ObserveRejection} with code `OBSERVER_ID_CONFLICT` and
   * sends nothing.
   */
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
  /**
   * Removes the observer (`unobserve`). It is not replayed on reconnect.
   * After the engine refused the `observe`, it sends nothing. It resolves at
   * once in every case and awaits no answer from the engine, also when it is
   * called before the answer to the `observe` arrives.
   */
  dispose(): Promise<void>
}

/**
 * One call of a function, as its host delivers it: what the handler was
 * called with and what it returned (or threw). Observers observe `output`,
 * the handler's return value.
 */
export interface Call<I = unknown, O = unknown> {
  function_id: string
  invocation_id: string
  status: 'success' | 'error'
  input: I
  /** The handler's return value: what observers observe. */
  output?: O
  /** What the handler threw. */
  error?: { code: string; message: string; stacktrace?: string }
  /** Who made the call; absent for a call made outside any handler. */
  caller?: Caller
  metadata?: Record<string, unknown>
}

/**
 * An observer as the worker that hosts its function sees it (the hooks,
 * `FunctionRef.observers()`): its args, filters, target, capture and `on`.
 * Who observes, the observer's metadata and its policy stay with the engine.
 */
export interface Observer {
  readonly id: string
  /** The observed function (one this worker hosts). */
  readonly function_id: string
  /** The observer's args, validated against the function's `args_format` by the engine. */
  readonly args: Record<string, unknown>
  /** The observer's filters, normalized. */
  readonly filters: Filter
  /** The target function id. */
  readonly target: string
  readonly on: 'success' | 'error' | 'complete'
  readonly capture: Capture[]
  /**
   * Whether `call` reaches this observer: its `on` accepts the call's
   * `status` (absent reads as `'success'`) and its filters match the call.
   */
  matches(call: Partial<Call>): boolean
  /**
   * Resolves once the SDK has acked the observer. A call made after it
   * resolves reaches an active observer, so a provider sends an observer its
   * initial state from here: a call of its own function with
   * `metadata: { for_observer: obs.id }`, which the function's match hook
   * gives to this observer alone, delivered like every call. Rejects with an
   * {@link ObserveRejection} when the observer is rejected first.
   *
   * @example
   * ```typescript
   * iii.onObserverAdded('tmux::on-changed', (obs) => {
   *   void obs
   *     .accepted()
   *     .then(() =>
   *       iii.call({
   *         function_id: 'tmux::on-changed',
   *         payload: stateFor(obs.args),
   *         metadata: { for_observer: obs.id },
   *         action: CallAction.Void(),
   *       }),
   *     )
   *     .catch(() => {
   *       // a rejected observer gets nothing
   *     })
   * })
   * iii.onObserverMatch('tmux::on-changed', (obs, call) =>
   *   call.metadata?.for_observer ? call.metadata.for_observer === obs.id : obs.args.session === call.input.session,
   * )
   * ```
   */
  accepted(): Promise<void>
  /**
   * Late rejection of an active observer (e.g. after a config reload). It
   * stops receiving calls at once, its `onObserverRemoved` hooks run, and it
   * stays rejected across reconnects until the engine adds it again.
   */
  reject(code: string, message: string): void
}

/**
 * Runs when an observer of a function is added, before the SDK acks it.
 * Throw to reject the observer: an {@link ObserveRejection} sets the code,
 * anything else is `OBSERVE_REJECTED`; after 30 s it is rejected with
 * `OWNER_TIMEOUT`.
 */
export type AddedHook = (obs: Observer) => void | Promise<void>

/** Runs once for every accepted observer when it is removed. An error it throws is logged. */
export type RemovedHook = (obs: Observer) => void | Promise<void>

/** A match key, or several: an observer wants a call when they share one. */
export type Key = string | readonly string[]

/** `onObserverMatch` as a predicate: whether this observer wants this call. */
export type MatchPredicate<I = unknown, O = unknown> = (obs: Observer, call: Call<I, O>) => boolean

/**
 * `onObserverMatch` as keys: an observer wants a call when they share a key,
 * and the SDK keeps an index instead of asking every observer.
 */
export interface MatchKeys<I = unknown, O = unknown> {
  /** What this observer listens for, computed once when it is accepted. */
  observer: (obs: Observer) => Key
  /** What this call is. */
  call: (call: Call<I, O>) => Key
}

/**
 * Which observers want each call of a function (`onObserverMatch`). By
 * default every observer wants every call. A predicate is asked once per
 * active observer per call (a throw counts as no match). Keys are computed
 * once per observer, when it is accepted, and once per call. The observers'
 * `on`, filters, policy and `deliver` still apply. A host that sends one
 * observer something of its own (its initial state) calls the function with
 * `metadata: { for_observer: obs.id }`; the hook gives such a call to that
 * observer alone (a predicate compares the ids; keys file each observer under
 * `for_observer:<id>` too).
 */
export type MatchHook<I = unknown, O = unknown> = MatchPredicate<I, O> | MatchKeys<I, O>

/**
 * Delivers each call of a function itself (`onObserverDeliver`): it receives
 * the call and the observers that matched it (a snapshot taken when the call
 * started), and what it returns (or throws) is what the caller gets back. A
 * thrown {@link ObserveRejection}, or the error of a call it made, reaches the
 * caller with its code; any other error as `invocation_failed`. iii delivers
 * nothing for that function; the hook calls through observers
 * (`worker.call({ observer, payload })`) and applies their filters with
 * `obs.matches(call)`. It runs inside the function's invocation, so the calls
 * it makes have that invocation as their caller.
 */
export type DeliverHook<I = unknown, O = unknown, R = O> = (
  call: Call<I, O>,
  observers: readonly Observer[],
) => R | Promise<R>

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
