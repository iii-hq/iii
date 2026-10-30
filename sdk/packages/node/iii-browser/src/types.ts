import type { ChannelReader, ChannelWriter } from './channels'
import type { RegistrationRejectedError } from './errors'
import type { IIIConnectionState } from './iii-constants'
import type { CallRequest, RegisterFunctionMessage, StreamChannelRef } from './iii-types'
import type { FunctionObservers } from './observable'
import type {
  AddedHook,
  Call,
  DeliverHook,
  InvocationContext,
  MatchHook,
  MatchKeys,
  MatchPredicate,
  Observer,
  ObserverHandle,
  ObserverOptions,
  ObserveSource,
  RemovedHook,
  Target,
} from './observe-types'

/**
 * Handler of a registered function. Receives the invocation payload and its
 * metadata (`CallRequest.metadata` of the call, or an observer's metadata
 * sidecar on an activation) and returns the result (or a promise of it). The
 * return value is what the function's observers observe.
 *
 * @typeParam TInput - Type of the invocation payload.
 * @typeParam TOutput - Type of the return value.
 *
 * @example
 * ```typescript
 * const handler: RemoteFunctionHandler<{ name: string }, { message: string }> =
 *   async (data) => ({ message: `Hello, ${data.name}!` })
 * ```
 */
// biome-ignore lint/suspicious/noExplicitAny: generic defaults require any for contravariant compatibility
export type RemoteFunctionHandler<TInput = any, TOutput = any> = (
  data: TInput,
  metadata?: unknown,
) => TOutput | Promise<TOutput>

// biome-ignore lint/suspicious/noExplicitAny: generic default requires any for contravariant compatibility
export type Invocation<TOutput = any> = {
  resolve: (data: TOutput) => void
  // biome-ignore lint/suspicious/noExplicitAny: error can be any type
  reject: (error: any) => void
}

/** Internal handler type: the payload and the invocation's metadata. */
// biome-ignore lint/suspicious/noExplicitAny: generic defaults require any for contravariant compatibility
export type InternalFunctionHandler<TInput = any, TOutput = any> = (
  data: TInput,
  metadata?: unknown,
) => Promise<TOutput>

export type RemoteFunctionData = {
  message: RegisterFunctionMessage
  handler?: InternalFunctionHandler
  /** Observers of this function as the engine pushed them, and local dispatch state. */
  observe: FunctionObservers
  /** When another connection hosts the id, retry the registration until it is free. */
  standby: boolean
  /** The connection (`epoch`) the registration was sent on; registrations that need `observe.v2` wait for it. */
  sentEpoch?: number
  /** Waiting, as a `standby`, for another connection to release the id. */
  standingBy?: boolean
}

export type RemoteServiceFunctionData = {
  message: Omit<RegisterFunctionMessage, 'serviceId'>
  handler: RemoteFunctionHandler
}

export type RegisterFunctionInput = Omit<RegisterFunctionMessage, 'message_type'>
/**
 * Options of {@link ISdk.registerFunction}: what describes the function to
 * the engine (description, formats, metadata, `args_format`, `private`,
 * `observable`), and what the SDK does with it (`standby`, `localDispatch`).
 * What the worker does when the function is observed is set with hooks.
 */
export type RegisterFunctionOptions = Omit<RegisterFunctionMessage, 'message_type' | 'id'> & {
  /**
   * When another connection already hosts the id, wait and take over when it
   * leaves: the registration is retried every second until it succeeds or
   * the function is unregistered (default `false`).
   */
  standby?: boolean
  /**
   * Calls this worker makes to its own function run in-process once the
   * engine confirmed it (default `true`). Set `false` for a handler that must
   * always run through the engine.
   */
  localDispatch?: boolean
}

export interface ISdk {
  /**
   * Registers a new function with a local handler that runs in the browser
   * session. HTTP invocation configs are a Node.js SDK feature. The handler's
   * return value is what the function's observers observe.
   *
   * A function that sets `args_format`, `private: true`, `observable: false`,
   * an `onObserverMatch` hook or an `onObserverDeliver` hook needs an engine
   * with `observe.v2`: it is registered once the connection's engine
   * advertised it, and against an older engine it stays unregistered (logged
   * once, `UNSUPPORTED`).
   *
   * @param functionId - Unique function identifier
   * @param handler - Handler executed when the function is invoked
   * @param options - Optional {@link RegisterFunctionOptions}: description, request/response formats, metadata, `args_format`, `private`, `observable`, `standby`, `localDispatch`
   * @returns A {@link FunctionRef}: the hook methods with the id filled in, and `unregister()`
   *
   * @example
   * ```typescript
   * // Local handler
   * const ref = worker.registerFunction(
   *   'greet',
   *   async (data: { name: string }) => ({ message: `Hello, ${data.name}!` }),
   *   { description: 'Returns a greeting' },
   * )
   *
   * // An event source: observed by everyone, called only by this page, set up
   * // for each observer by its args
   * worker
   *   .registerFunction('ui::on-click', (click: Click) => click, { args_format: ClickArgs, private: true })
   *   .onObserverAdded((obs) => targets.retain(obs.args.selector as string))
   *   .onObserverRemoved((obs) => targets.release(obs.args.selector as string))
   *
   * // Later, remove the function
   * ref.unregister()
   * ```
   */
  // biome-ignore lint/suspicious/noExplicitAny: the handler's types default to any so untyped handlers type-check
  registerFunction<I = any, O = any>(
    functionId: string,
    handler: RemoteFunctionHandler<I, O>,
    options?: RegisterFunctionOptions,
  ): FunctionRef<I, O>

  /**
   * Invokes a function: by id, or the target of an observer of a function
   * this worker hosts (`{ observer, payload }`).
   * <!-- docs:expand-params -->
   *
   * @param request - The call request: `function_id` (or `observer`), `payload`, and optional action/timeout
   * @returns The result of the function
   *
   * @example
   * ```typescript
   * // Synchronous invocation
   * const result = await worker.call<{ name: string }, { message: string }>({
   *   function_id: 'greet',
   *   payload: { name: 'World' },
   *   timeoutMs: 5000,
   * })
   * console.log(result.message) // "Hello, World!"
   *
   * // Fire-and-forget
   * await worker.call({
   *   function_id: 'send-email',
   *   payload: { to: 'user@example.com' },
   *   action: CallAction.Void(),
   * })
   *
   * // Enqueue for async processing (the queue must be declared in the
   * // queue worker's queue_configs)
   * const receipt = await worker.call({
   *   function_id: 'process-order',
   *   payload: { orderId: '123' },
   *   action: CallAction.Enqueue({ queue: 'orders' }),
   * })
   *
   * // Through an observer: runs its target, with its policy and metadata
   * const response = await worker.call({ observer: route.obs, payload: request })
   * ```
   */
  // biome-ignore lint/suspicious/noExplicitAny: TOutput defaults to any so untyped calls type-check (the engine cannot express the return type statically)
  call<TInput = unknown, TOutput = any>(request: CallRequest<TInput>): Promise<TOutput>

  /**
   * Invokes a function using a request object.
   *
   * @deprecated Use {@link ISdk.call}, the same function.
   */
  // biome-ignore lint/suspicious/noExplicitAny: TOutput defaults to any so untyped calls type-check (the engine cannot express the return type statically)
  trigger<TInput = unknown, TOutput = any>(request: CallRequest<TInput>): Promise<TOutput>

  /**
   * Observes a function: every call of `source` that passes its `filters`
   * runs `target`, a registered function. Returns synchronously (also before
   * the connection is up); the observer is replayed with the same id on
   * reconnect. Errors show up as state `error` and reject `ready()`; against
   * an engine without observe that is `UNSUPPORTED`.
   *
   * @param source - What is observed: a function id, or `function_id`, `args` (read by the worker that hosts the function), `filters`, `on`, `capture`, `deliver`.
   * @param target - The function to invoke.
   * @param options - `id`, `max_fires`, `expires_in_ms`, `max_in_flight`, `metadata`.
   *
   * @example
   * ```typescript
   * worker.registerFunction('ui::order-shipped', async ({ input }: Observed) => toast(input))
   *
   * // every successful call of orders::ship
   * worker.observe('orders::ship', 'ui::order-shipped')
   *
   * // only this customer's
   * const shipped = worker.observe(
   *   { function_id: 'orders::ship', filters: { 'input.customer_id': { $eq: customerId } } },
   *   'ui::order-shipped',
   * )
   * await shipped.ready()
   * ```
   */
  observe(source: string | ObserveSource, target: Target, options?: ObserverOptions): ObserverHandle

  /**
   * Runs `hook` when an observer of any function this worker hosts is added,
   * before the SDK acks it; with a function id, for that function only. When
   * both are set, the worker's runs first. Throw to reject the observer.
   * `null` removes the hook. Hooks set by id stay in place across
   * `unregister()` and a later registration of the same id.
   *
   * A hook set while the function already has active observers runs for each
   * of them at once (a throw late-rejects that observer), and the SDK logs a
   * warning.
   *
   * @example
   * ```typescript
   * // every function this page hosts: keep the event stream open while anything is observed
   * worker.onObserverAdded(async () => { await events.retain() })
   * // one function, by id
   * worker.onObserverAdded('ui::on-click', (obs) => targets.retain(obs.args.selector as string))
   * ```
   */
  onObserverAdded(hook: AddedHook | null): ISdk
  onObserverAdded(functionId: string, hook: AddedHook | null): ISdk

  /**
   * Runs `hook` once for every accepted observer when it is removed (on
   * `unobserve`, disposal, unregister, late rejection); with a function id,
   * for that function only. When both are set, the function's runs first. An
   * error it throws is logged. `null` removes the hook.
   */
  onObserverRemoved(hook: RemovedHook | null): ISdk
  onObserverRemoved(functionId: string, hook: RemovedHook | null): ISdk

  /**
   * Says which observers want each call of a function. By default every
   * observer wants every call. A predicate `(obs, call) => boolean` is asked
   * once per active observer per call; `{ observer, call }` key functions are
   * computed once per observer, when it is accepted, and once per call, and an
   * observer wants a call when they share a key (the SDK keeps an index). The
   * observers' `on`, filters, policy and `deliver` still apply. A hook set
   * while observers are active rebuilds the index from them. `null` removes
   * the hook.
   *
   * A function with a match hook needs an engine with `observe.v2`: against
   * an older one it stays unregistered, and the SDK logs `UNSUPPORTED` once.
   *
   * @example
   * ```typescript
   * // an observer of selector X gets the clicks on X
   * worker.onObserverMatch('ui::on-click', (obs, call) => obs.args.selector === call.input.selector)
   *
   * // many observers: keys, one lookup per key
   * worker.onObserverMatch('ui::on-click', {
   *   observer: (obs) => obs.args.selector as string,
   *   call: (call) => call.input.selector,
   * })
   * ```
   */
  // biome-ignore lint/suspicious/noExplicitAny: the call's types default to any so untyped hooks read their input
  onObserverMatch<I = any, O = any>(functionId: string, hook: MatchPredicate<I, O> | null): ISdk
  // biome-ignore lint/suspicious/noExplicitAny: the call's types default to any so untyped key functions read their input
  onObserverMatch<I = any, O = any>(functionId: string, hook: MatchKeys<I, O>): ISdk
  // biome-ignore lint/suspicious/noExplicitAny: the call's types default to any so untyped hooks read their input
  onObserverMatch<I = any, O = any>(functionId: string, hook: MatchHook<I, O> | null): ISdk

  /**
   * Delivers each call of a function itself: `hook` receives the call and the
   * observers that matched it (a snapshot taken when the call started), and
   * what it returns (or throws) is what the caller gets back. iii delivers
   * nothing for that function: the hook calls through observers and applies
   * their filters with `obs.matches(call)`. Without it, iii delivers every
   * call to the matching observers whose `on` and filters accept it, and the
   * caller gets the handler's result. `null` removes the hook.
   *
   * A function with a deliver hook needs an engine with `observe.v2`: against
   * an older one it stays unregistered, and the SDK logs `UNSUPPORTED` once.
   *
   * The hook runs in the function's invocation: up to its first `await` in
   * its scope, and after it as the invocation the page falls back to while it
   * is the only one running (see {@link ISdk.withInvocation}).
   *
   * @example
   * ```typescript
   * worker.onObserverDeliver('ui::on-command', async (call) => {
   *   const handler = commands.match(call.input)
   *   if (!handler) return { handled: false }
   *   if (!handler.obs.matches(call)) return { handled: false }
   *   return worker.call({ observer: handler.obs, payload: call.input })
   * })
   * ```
   */
  // biome-ignore lint/suspicious/noExplicitAny: the call's types default to any so untyped hooks read their input
  onObserverDeliver<I = any, O = any, R = O>(functionId: string, hook: DeliverHook<I, O, R> | null): ISdk

  /** The active observers of a function this worker hosts (empty for any other). */
  observers(functionId: string): readonly Observer[]

  /**
   * Whether a call of a function this worker hosts would reach anyone.
   * Without a call: any active observer. With a call, on a function matched
   * by keys: an observer filed under one of the call's keys. On any other
   * function the call is not read.
   */
  isObserved(functionId: string, call?: Partial<Call>): boolean

  /**
   * The invocation the calling code runs in (remote or local handler, or the
   * function's deliver hook), or `undefined` outside any handler.
   *
   * Browsers have no `AsyncLocalStorage`: the SDK knows the invocation while
   * the handler runs synchronously, up to its first `await`; after it this
   * returns `undefined`. Read it before awaiting, and re-enter it with
   * {@link ISdk.withInvocation} for calls made after.
   *
   * Code after an `await`, outside {@link ISdk.withInvocation}, runs outside any
   * invocation, but its calls are not treated as new chains: they carry the
   * `observe_depth` of the deepest invocation running on this page, so the
   * engine's loop guard (`max_observe_depth`) still stops an observer cycle
   * whose target awaits before calling back. With several invocations
   * running this over-counts, and a legitimate chain may stop early. They
   * carry a `caller` and `traceparent` only when that is unambiguous (one
   * invocation is running, and it is an observe activation or its function's
   * deliver hook is running); otherwise none. An invocation that never
   * settles stops counting as running after 10 minutes (a local call: at its
   * timeout). Local calls nest the same way: a self-recursion that awaits
   * before each call goes through the engine past 64 levels, while calls
   * started together count as one level. A limitation: local calls started in
   * separate ticks while earlier ones still run also look nested, so a long,
   * wide fan-out may send some through the engine (same results). While the
   * socket is closed none is: a function this worker hosts then runs here up
   * to 256 local calls deep, and past that the call fails at once with
   * `LOCAL_DEPTH_EXCEEDED` (a runaway recursion must not freeze the page).
   *
   * @example
   * ```typescript
   * worker.registerFunction('ui::checkout', async (cart) => {
   *   const inv = worker.currentInvocation() // { function_id, invocation_id, caller, observe_depth, ... }
   *   const quote = await fetchQuote(cart)
   *   return worker.withInvocation(inv, () => worker.call({ function_id: 'orders::create', payload: quote }))
   * })
   * ```
   */
  currentInvocation(): InvocationContext | undefined

  /**
   * Runs `fn` inside `invocation` (what {@link ISdk.currentInvocation}
   * returned), and returns what `fn` returns. Calls `fn` makes before its own
   * first `await` (`call`, `currentInvocation`) belong to that invocation:
   * they carry its caller, `observe_depth` and `traceparent`. When `fn`
   * returns a promise, the invocation counts as running until it settles, so
   * `fn`'s code after an `await` gets the fallback below. `undefined` runs
   * `fn` outside any invocation.
   *
   * Code after an `await`, outside `withInvocation`, runs outside any
   * invocation, but its calls are not treated as new chains: they carry the
   * `observe_depth` of the deepest invocation running on this page, so the
   * engine's loop guard (`max_observe_depth`) still stops an observer cycle
   * whose target awaits before calling back. With several invocations
   * running this over-counts, and a legitimate chain may stop early. They
   * carry a `caller` and `traceparent` only when that is unambiguous (one
   * invocation is running, and it is an observe activation or its function's
   * deliver hook is running); otherwise none. An invocation that never
   * settles stops counting as running after 10 minutes (a local call: at its
   * timeout). Local calls nest the same way: a self-recursion that awaits
   * before each call goes through the engine past 64 levels, while calls
   * started together count as one level. A limitation: local calls started in
   * separate ticks while earlier ones still run also look nested, so a long,
   * wide fan-out may send some through the engine (same results). While the
   * socket is closed none is: a function this worker hosts then runs here up
   * to 256 local calls deep, and past that the call fails at once with
   * `LOCAL_DEPTH_EXCEEDED` (a runaway recursion must not freeze the page).
   */
  withInvocation<T>(invocation: InvocationContext | undefined, fn: () => T): T

  /**
   * Gracefully shutdown the iii, cleaning up all resources.
   *
   * @example
   * ```typescript
   * await worker.shutdown()
   * ```
   */
  shutdown(): Promise<void>

  /**
   * Subscribe to connection-state transitions. The handler is fired immediately
   * with the current state, then on every transition. Multiple listeners are
   * supported. Returns an unsubscribe function.
   *
   * @param handler - Callback invoked with the new connection state whenever it changes.
   * @returns An unsubscribe function that stops further callbacks.
   *
   * @example
   * ```typescript
   * const unsub = worker.addConnectionStateListener((state) => {
   *   console.log('connection state:', state)
   * })
   *
   * // Later, stop receiving updates
   * unsub()
   * ```
   */
  addConnectionStateListener(handler: (state: IIIConnectionState) => void): () => void

  /**
   * The fatal registration rejection that terminated this connection, if any
   * (e.g. a `WORKER_NAMESPACE_CONFLICT`); `undefined` while healthy. Mirrors the
   * Node/Python/Rust SDKs.
   */
  getFatalError(): RegistrationRejectedError | undefined
}

/**
 * Handle returned by {@link ISdk.registerFunction}: the client's hook
 * methods with the function id filled in. Each method returns the handle, so
 * calls chain.
 *
 * @example
 * ```typescript
 * worker.registerFunction('ui::on-click', (click: Click) => click, { private: true })
 *   .onObserverAdded((obs) => targets.retain(obs.args.selector as string))
 *   .onObserverRemoved((obs) => targets.release(obs.args.selector as string))
 * ```
 */
// biome-ignore lint/suspicious/noExplicitAny: refs of differently typed functions share one type (`FunctionRef[]`)
export interface FunctionRef<I = any, O = any> {
  /** The unique function identifier. */
  readonly id: string
  /** {@link ISdk.onObserverAdded} for this function. */
  onObserverAdded(hook: AddedHook | null): FunctionRef<I, O>
  /** {@link ISdk.onObserverRemoved} for this function. */
  onObserverRemoved(hook: RemovedHook | null): FunctionRef<I, O>
  /** {@link ISdk.onObserverMatch} for this function. */
  onObserverMatch(hook: MatchPredicate<I, O> | null): FunctionRef<I, O>
  onObserverMatch(hook: MatchKeys<I, O>): FunctionRef<I, O>
  onObserverMatch(hook: MatchHook<I, O> | null): FunctionRef<I, O>
  /** {@link ISdk.onObserverDeliver} for this function. */
  onObserverDeliver<R = O>(hook: DeliverHook<I, O, R> | null): FunctionRef<I, O>
  /**
   * The active observers of this registration, as this host sees them (none
   * once the function is unregistered, also after the id is registered again).
   */
  observers(): readonly Observer[]
  /**
   * {@link ISdk.isObserved} for this registration: skip building an event
   * nobody receives. Without a call it is one load of this registration's
   * active set.
   */
  isObserved(call?: Partial<Call<I>>): boolean
  /** Removes this function from the engine. Its hooks stay in place for a later registration of the id. */
  unregister(): void
}

/**
 * A streaming channel pair for worker-to-worker data transfer. Created via
 * the `createChannel` helper from `iii-browser-sdk/helpers`.
 */
export type Channel = {
  /** Writer end of the channel. */
  writer: ChannelWriter
  /** Reader end of the channel. */
  reader: ChannelReader
  /** Serializable reference to the writer (can be sent to other workers). */
  writerRef: StreamChannelRef
  /** Serializable reference to the reader (can be sent to other workers). */
  readerRef: StreamChannelRef
}

/**
 * The HTTP request an `http::on-request` route's target receives.
 *
 * @typeParam TBody - Type of the parsed request body.
 */
export type ApiRequest<TBody = unknown> = {
  path_params: Record<string, string>
  query_params: Record<string, string | string[]>
  body: TBody
  headers: Record<string, string | string[]>
  method: string
}

/**
 * The response an `http::on-request` route's target returns.
 *
 * @typeParam TStatus - HTTP status code literal type.
 * @typeParam TBody - Type of the response body.
 *
 * @example
 * ```typescript
 * const response: ApiResponse = {
 *   status_code: 200,
 *   headers: { 'content-type': 'application/json' },
 *   body: { message: 'ok' },
 * }
 * ```
 */
export type ApiResponse<TStatus extends number = number, TBody = string | Record<string, unknown>> = {
  /** HTTP status code. */
  status_code: TStatus
  /** Response headers. */
  headers?: Record<string, string>
  /** Response body. */
  body?: TBody
}
