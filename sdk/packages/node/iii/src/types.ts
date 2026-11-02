import type { HttpInvocationConfig } from '@iii-dev/helpers/http'
import type { ChannelReader, ChannelWriter } from './channels'
import type { RegistrationRejectedError } from './errors'
import type { IIIConnectionState } from './iii-constants'
import type { CallRequest, JsonValue, RegisterFunctionMessage, StreamChannelRef } from './iii-types'
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
 * Handler of a registered function. Receives the invocation payload and an
 * optional per-invocation `metadata` value, and returns the result (or a
 * promise of it). The return value is what the function's observers observe.
 *
 * `metadata` is arbitrary JSON travelling on a separate channel from the
 * payload. It is `undefined` when the caller did not attach any. Existing
 * single-argument handlers keep working; they ignore the extra argument.
 *
 * @typeParam TInput - Type of the invocation payload.
 * @typeParam TOutput - Type of the return value.
 *
 * @example
 * ```typescript
 * const handler: RemoteFunctionHandler<{ name: string }, { message: string }> =
 *   async (data, metadata) => ({ message: `Hello, ${data.name}!` })
 * ```
 */
// biome-ignore lint/suspicious/noExplicitAny: generic defaults require any for contravariant compatibility
export type RemoteFunctionHandler<TInput = any, TOutput = any> = (
  data: TInput,
  metadata?: JsonValue,
) => TOutput | Promise<TOutput>

// biome-ignore lint/suspicious/noExplicitAny: generic default requires any for contravariant compatibility
export type Invocation<TOutput = any> = {
  resolve: (data: TOutput) => void
  // biome-ignore lint/suspicious/noExplicitAny: error can be any type
  reject: (error: any) => void
  /**
   * Target function_id for the pending invocation, preserved so timeout and
   * error-wrapping paths can name the function that tripped without needing
   * to plumb it through every call site.
   */
  function_id?: string
}

/** How a handler finished: what it returned, or what it threw. */
export type HandlerOutcome = { ok: true; value: unknown } | { ok: false; error: unknown }

/**
 * Internal handler type. Wraps the user {@link RemoteFunctionHandler} and adds
 * the trace context (traceparent/baggage) threaded internally by the SDK.
 * `metadata` mirrors the user-facing argument; trace context stays internal
 * and is not surfaced to the user handler. `settle` receives the handler's
 * outcome inside the invocation's span (the call's delivery runs there) and
 * gives what the wrapper resolves to; without it the wrapper resolves to the
 * handler's result.
 */
// biome-ignore lint/suspicious/noExplicitAny: generic defaults require any for contravariant compatibility
export type InternalFunctionHandler<TInput = any> = (
  data: TInput,
  metadata?: JsonValue,
  traceparent?: string,
  baggage?: string,
  settle?: (outcome: HandlerOutcome) => unknown,
) => Promise<unknown>

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
 * Options of {@link IIIClient.registerFunction}: what describes the function
 * to the engine (description, formats, metadata, `args_format`, `private`,
 * `observable`), and what the SDK does with it (`standby`, `localDispatch`).
 * What the worker does when the function is observed is set with hooks.
 */
export type RegisterFunctionOptions = Omit<RegisterFunctionMessage, 'message_type' | 'id'> & {
  /**
   * When another connection already hosts the id
   * (`FUNCTION_NAMESPACE_CONFLICT`), wait and take over when it leaves: the
   * registration is retried every second (±20 %) until the engine accepts it
   * or the function is unregistered, and the hooks run for every observer
   * once the engine confirms it (default `false`).
   */
  standby?: boolean
  /**
   * Calls this worker makes to its own function run in-process once the
   * engine confirmed it (default `true`). Set `false` for a handler that must
   * always run through the engine.
   */
  localDispatch?: boolean
}

export interface IIIClient {
  /**
   * Registers a new function with a local handler or an HTTP invocation config.
   * The handler's return value is what the function's observers observe; an
   * event source's handler returns the event it is called with.
   *
   * Every call of the function, local or routed, runs the handler and then
   * goes to the observers that want it (`onObserverMatch`; by default all of
   * them): the function's `onObserverDeliver` hook delivers it, or iii does,
   * to those whose `on` and filters match. iii evaluates `on` and filters
   * where the call is: the engine for a call it routes to this worker, and
   * the SDK for a call this worker makes to its own function in-process,
   * through an index of the filters' equality conditions. What the worker
   * does when the function is observed is set with the hooks (`onObserverAdded`,
   * `onObserverRemoved`, `onObserverMatch`, `onObserverDeliver`), by id or on
   * the returned {@link FunctionRef}. The SDK acks no observer before this
   * call returns, so a hook set by id before it, chained on its result, or
   * set later in the same synchronous block runs for every observer.
   *
   * A function that sets `args_format`, `private: true`, `observable: false`,
   * an `onObserverMatch` hook or an `onObserverDeliver` hook needs an engine
   * with `observe.v2`: it is registered once the connection's engine
   * advertised it, and against an older engine it stays unregistered (logged
   * once, `UNSUPPORTED`).
   *
   * @param functionId - Unique identifier for the function.
   * @param handler - Handler for local execution, or an HTTP invocation config for external functions (Lambda, Cloudflare Workers, etc.).
   * @param options - Optional {@link RegisterFunctionOptions}: description, request/response formats, metadata, `args_format`, `private`, `observable`, `standby`, `localDispatch`.
   * @returns A {@link FunctionRef}: the hook methods with the id filled in, and `unregister()`.
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
   * // An event source: observed by everyone, called only by this worker,
   * // set up for each observer by its args
   * worker
   *   .registerFunction('cron::on-tick', (tick: CronTick) => tick, { args_format: CronArgs, private: true })
   *   .onObserverAdded((obs) => scheduler.retain(obs.args.expression as string))
   *   .onObserverRemoved((obs) => scheduler.release(obs.args.expression as string))
   *
   * // HTTP invocation
   * const lambdaRef = worker.registerFunction(
   *   'external::my-lambda',
   *   {
   *     url: 'https://abc123.lambda-url.us-east-1.on.aws',
   *     method: 'POST',
   *     timeout_ms: 30_000,
   *     auth: { type: 'bearer', token_key: 'LAMBDA_AUTH_TOKEN' },
   *   },
   *   { description: 'Proxied Lambda function' },
   * )
   *
   * // Later, remove the function
   * ref.unregister()
   * ```
   */
  // biome-ignore lint/suspicious/noExplicitAny: the handler's types default to any so untyped handlers type-check
  registerFunction<I = any, O = any>(
    functionId: string,
    handler: RemoteFunctionHandler<I, O> | HttpInvocationConfig,
    options?: RegisterFunctionOptions,
  ): FunctionRef<I, O>

  /**
   * Invokes a function: by id, or the target of an observer of a function
   * this worker hosts (`{ observer, payload }`).
   * <!-- docs:expand-params -->
   *
   * @param request - The call request: `function_id` (or `observer`), `payload`, and optional action/timeout.
   * @returns The result of the function.
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
   * // Through an observer, in the function's deliver hook: runs its target,
   * // with its policy and metadata
   * const response = await worker.call({ observer: route.obs, payload: request })
   * ```
   */
  // biome-ignore lint/suspicious/noExplicitAny: TOutput defaults to any so untyped calls type-check (the engine cannot express the return type statically)
  call<TInput = unknown, TOutput = any>(request: CallRequest<TInput>): Promise<TOutput>

  /**
   * Invokes a function using a request object.
   *
   * @deprecated Use {@link IIIClient.call}, the same function.
   */
  // biome-ignore lint/suspicious/noExplicitAny: TOutput defaults to any so untyped calls type-check (the engine cannot express the return type statically)
  trigger<TInput = unknown, TOutput = any>(request: CallRequest<TInput>): Promise<TOutput>

  /**
   * Observes a function: every call of `source` that passes its `filters`
   * runs `target`, a registered function. Returns synchronously (also before
   * the connection is up); the observer is replayed with the same id on
   * reconnect. Errors show up as state `error` and reject `ready()`. An
   * `error` answer to its `observe` is the engine's refusal, the handle's
   * final answer: the engine holds no observer for it, it is not replayed,
   * and its `dispose()` sends nothing.
   *
   * @param source - What is observed: a function id, or `function_id`, `args` (read by the worker that hosts the function), `filters`, `on`, `capture`, `deliver`.
   * @param target - The function to invoke.
   * @param options - `id`, `max_fires`, `expires_in_ms`, `max_in_flight`, `metadata`.
   *
   * @example
   * ```typescript
   * // every successful call of orders::create runs alerts::big-order
   * worker.observe('orders::create', 'alerts::big-order')
   *
   * // only large orders
   * worker.observe(
   *   { function_id: 'orders::create', filters: { 'input.total': { $gte: 500 } } },
   *   'alerts::big-order',
   * )
   *
   * // an event source with args for its worker: one tick every five minutes
   * const ticks = worker.observe(
   *   { function_id: 'cron::on-tick', args: { expression: '0 *\/5 * * * * *' } },
   *   'reports::refresh',
   * )
   * await ticks.ready()
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
   * // every function this worker hosts: keep the event stream open while anything is observed
   * worker.onObserverAdded(async () => { await events.retain() })
   * // one function, by id
   * worker.onObserverAdded('database::on-row-changed', async (obs) => {
   *   await capture.install(obs.args.db, obs.args.table)
   * })
   * ```
   */
  onObserverAdded(hook: AddedHook | null): IIIClient
  onObserverAdded(functionId: string, hook: AddedHook | null): IIIClient

  /**
   * Runs `hook` once for every accepted observer when it is removed (on
   * `unobserve`, disposal, unregister, late rejection); with a function id,
   * for that function only. When both are set, the function's runs first. An
   * error it throws is logged. `null` removes the hook.
   */
  onObserverRemoved(hook: RemovedHook | null): IIIClient
  onObserverRemoved(functionId: string, hook: RemovedHook | null): IIIClient

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
   * // an observer of expression X gets the ticks of expression X
   * worker.onObserverMatch('cron::on-tick', (obs, call) => obs.args.expression === call.input.expression)
   *
   * // many observers: keys, one lookup per key
   * worker.onObserverMatch('database::on-row-changed', {
   *   observer: (obs) => `${obs.args.table}|${obs.args.op}`,
   *   call: (call) => `${call.input.table}|${call.input.op}`,
   * })
   * ```
   */
  // biome-ignore lint/suspicious/noExplicitAny: the call's types default to any so untyped hooks read their input
  onObserverMatch<I = any, O = any>(functionId: string, hook: MatchPredicate<I, O> | null): IIIClient
  // biome-ignore lint/suspicious/noExplicitAny: the call's types default to any so untyped key functions read their input
  onObserverMatch<I = any, O = any>(functionId: string, hook: MatchKeys<I, O>): IIIClient
  // biome-ignore lint/suspicious/noExplicitAny: the call's types default to any so untyped hooks read their input
  onObserverMatch<I = any, O = any>(functionId: string, hook: MatchHook<I, O> | null): IIIClient

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
   * @example
   * ```typescript
   * worker.onObserverDeliver('http::on-request', async (call) => {
   *   const route = routes.match(call.input)
   *   if (!route) return { status_code: 404 }
   *   if (!route.obs.matches(call)) return { status_code: 422 }
   *   return worker.call({ observer: route.obs, payload: call.input })
   * })
   * ```
   */
  // biome-ignore lint/suspicious/noExplicitAny: the call's types default to any so untyped hooks read their input
  onObserverDeliver<I = any, O = any, R = O>(functionId: string, hook: DeliverHook<I, O, R> | null): IIIClient

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
   */
  currentInvocation(): InvocationContext | undefined

  /**
   * Gracefully shutdown the iii, cleaning up all resources.
   *
   * @example
   * ```typescript
   * process.on('SIGTERM', async () => {
   *   await worker.shutdown()
   *   process.exit(0)
   * })
   * ```
   */
  shutdown(): Promise<void>

  /**
   * The current WebSocket connection state. `'failed'` is terminal: it follows
   * a fatal registration rejection (see {@link getFatalError}). Mirrors the
   * Python/Rust SDKs' `get_connection_state()`.
   */
  getConnectionState(): IIIConnectionState

  /**
   * Engine address this worker resolved to: the explicit `registerWorker`
   * argument, else `III_URL`, else `ws://127.0.0.1:49134`.
   */
  getAddress(): string

  /**
   * The fatal registration rejection that terminated this connection, if any
   * (e.g. a `WORKER_NAMESPACE_CONFLICT`); `undefined` while healthy. Mirrors the
   * Python (`_fatal_error`) and Rust (`fatal_error()`) SDKs.
   */
  getFatalError(): RegistrationRejectedError | undefined
}

/**
 * Handle returned by {@link IIIClient.registerFunction}: the client's hook
 * methods with the function id filled in. Each method returns the handle, so
 * calls chain.
 *
 * @example
 * ```typescript
 * iii.registerFunction('http::on-request', (request: HttpRequest) => request, { private: true })
 *   .onObserverAdded((obs) => routes.add(obs))
 *   .onObserverRemoved((obs) => routes.remove(obs.id))
 * ```
 */
// biome-ignore lint/suspicious/noExplicitAny: refs of differently typed functions share one type (`FunctionRef[]`)
export interface FunctionRef<I = any, O = any> {
  /** The unique function identifier. */
  readonly id: string
  /** {@link IIIClient.onObserverAdded} for this function. */
  onObserverAdded(hook: AddedHook | null): FunctionRef<I, O>
  /** {@link IIIClient.onObserverRemoved} for this function. */
  onObserverRemoved(hook: RemovedHook | null): FunctionRef<I, O>
  /** {@link IIIClient.onObserverMatch} for this function. */
  onObserverMatch(hook: MatchPredicate<I, O> | null): FunctionRef<I, O>
  onObserverMatch(hook: MatchKeys<I, O>): FunctionRef<I, O>
  onObserverMatch(hook: MatchHook<I, O> | null): FunctionRef<I, O>
  /** {@link IIIClient.onObserverDeliver} for this function. */
  onObserverDeliver<R = O>(hook: DeliverHook<I, O, R> | null): FunctionRef<I, O>
  /**
   * The active observers of this registration, as this host sees them (none
   * once the function is unregistered, also after the id is registered again).
   */
  observers(): readonly Observer[]
  /**
   * {@link IIIClient.isObserved} for this registration: skip building an event
   * nobody receives. Without a call it is one load of this registration's
   * active set.
   */
  isObserved(call?: Partial<Call<I>>): boolean
  /** Removes this function from the engine. Its hooks stay in place for a later registration of the id. */
  unregister(): void
}

/**
 * A streaming channel pair for worker-to-worker data transfer. Created via
 * the `createChannel` helper from `iii-sdk/helpers`.
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
 * Internal HTTP request shape that underlies `StreamRequest`.
 * <!-- docs:internal -->
 */
export type InternalHttpRequest<TBody = unknown> = {
  path_params: Record<string, string>
  query_params: Record<string, string | string[]>
  body: TBody
  headers: Record<string, string | string[]>
  method: string
  response: ChannelWriter
  request_body: ChannelReader
}

/**
 * Response object passed to streaming function handlers. Use `status()` and
 * `headers()` to set response metadata, write to `stream` for streaming
 * responses, and call `close()` when done.
 */
export type StreamResponse = {
  /** Set the HTTP status code. */
  status: (statusCode: number) => void
  /** Set response headers. */
  headers: (headers: Record<string, string>) => void
  /** Writable stream for the response body. */
  stream: NodeJS.WritableStream
  /** Close the response. */
  close: () => void
}

/**
 * Incoming streaming request received by a function that serves an HTTP route.
 *
 * @typeParam TBody - Type of the parsed request body.
 */
export type StreamRequest<TBody = unknown> = Omit<InternalHttpRequest<TBody>, 'response'>
