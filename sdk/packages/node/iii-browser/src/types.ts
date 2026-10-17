import type { ChannelReader, ChannelWriter } from './channels'
import type { RegistrationRejectedError } from './errors'
import type { IIIConnectionState } from './iii-constants'
import type {
  RegisterFunctionMessage,
  RegisterTriggerMessage,
  RegisterTriggerTypeMessage,
  StreamChannelRef,
  TriggerRequest,
} from './iii-types'
import type { FunctionObservers } from './observable'
import type {
  InvocationContext,
  ObserveHooks,
  Observer,
  ObserverHandle,
  ObserverOptions,
  ObserveSource,
  Target,
} from './observe-types'
import type { TriggerHandler } from './triggers'

/**
 * Async function handler for a registered function. Receives the invocation
 * payload and its metadata (`TriggerRequest.metadata` of the call, or an
 * observer's metadata sidecar on an activation) and returns the result.
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
export type RemoteFunctionHandler<TInput = any, TOutput = any> = (data: TInput, metadata?: unknown) => Promise<TOutput>

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
  /** Observers of this function as the engine pushed them, local dispatch state, hooks. */
  observe?: FunctionObservers
}

export type RemoteServiceFunctionData = {
  message: Omit<RegisterFunctionMessage, 'serviceId'>
  handler: RemoteFunctionHandler
}

/** @deprecated Trigger types are replaced by functions with an observe descriptor. */
export type RemoteTriggerTypeData = {
  message: RegisterTriggerTypeMessage
  // biome-ignore lint/suspicious/noExplicitAny: handler accepts any trigger config type
  handler: TriggerHandler<any>
}

/** @deprecated Use `iii.observe(source, target, options)` and its {@link ObserveSource}. */
export type RegisterTriggerInput = Omit<RegisterTriggerMessage, 'message_type' | 'id'>
export type RegisterFunctionInput = Omit<RegisterFunctionMessage, 'message_type'>
/**
 * Options of {@link ISdk.registerFunction}: description, request/response
 * formats, metadata, and the observe hooks.
 */
export type RegisterFunctionOptions = Omit<RegisterFunctionMessage, 'message_type' | 'id' | 'observe'> & {
  /**
   * Makes the function observable on its own terms: owner delivery
   * (`delivery: 'owner'`, `args_format`), and callbacks when an observer is
   * added or removed. Without it the function is still observable (auto
   * delivery, no args).
   */
  observe?: ObserveHooks
  /**
   * Calls this worker makes to its own function run in-process once the
   * engine confirmed it (default `true`). Set `false` for a handler that must
   * always run through the engine.
   */
  localDispatch?: boolean
}
/** @deprecated Register a function with an observe descriptor instead. */
export type RegisterTriggerTypeInput = Omit<RegisterTriggerTypeMessage, 'message_type'>

export interface ISdk {
  /**
   * Registers a new trigger. A trigger is a way to invoke a function when a certain event occurs.
   *
   * @deprecated Use {@link ISdk.observe} instead: `iii.observe({ function_id, args, filters }, target, { metadata })`.
   * Logs a warning once per page. The http, cron, queue, state, stream, pubsub and database workers
   * still publish trigger types: keep using `registerTrigger` for them until they ship source functions.
   * Migration guide: https://iii.dev/docs/upgrading/from-0-24-x
   * <!-- docs:expand-params -->
   * @param trigger - The trigger to register
   * @returns A trigger object that can be used to unregister the trigger
   *
   * @example
   * ```typescript
   * const trigger = worker.registerTrigger({
   *   type: 'cron',
   *   function_id: 'my-service::process-batch',
   *   config: { expression: '0 *\/5 * * * * *' },
   * })
   *
   * // Later, remove the trigger
   * trigger.unregister()
   * ```
   */
  registerTrigger(trigger: RegisterTriggerInput): Trigger

  /**
   * Registers a new function with a local async handler that runs in the
   * browser session. HTTP invocation configs are a Node.js SDK feature.
   * @param functionId - Unique function identifier
   * @param handler - Async handler executed when the function is invoked
   * @param options - Optional function registration options (description, request/response formats, metadata)
   * @returns A handle that can be used to unregister the function
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
   * // Later, remove the function
   * ref.unregister()
   * ```
   */
  registerFunction(functionId: string, handler: RemoteFunctionHandler, options?: RegisterFunctionOptions): FunctionRef

  /**
   * Invokes a function using a request object.
   * <!-- docs:expand-params -->
   *
   * @param request - The trigger request containing function_id, payload, and optional action/timeout
   * @returns The result of the function
   *
   * @example
   * ```typescript
   * // Synchronous invocation
   * const result = await worker.trigger<{ name: string }, { message: string }>({
   *   function_id: 'greet',
   *   payload: { name: 'World' },
   *   timeoutMs: 5000,
   * })
   * console.log(result.message) // "Hello, World!"
   *
   * // Fire-and-forget
   * await worker.trigger({
   *   function_id: 'send-email',
   *   payload: { to: 'user@example.com' },
   *   action: TriggerAction.Void(),
   * })
   *
   * // Enqueue for async processing (the queue must be declared in the
   * // queue worker's queue_configs)
   * const receipt = await worker.trigger({
   *   function_id: 'process-order',
   *   payload: { orderId: '123' },
   *   action: TriggerAction.Enqueue({ queue: 'orders' }),
   * })
   * ```
   */
  // biome-ignore lint/suspicious/noExplicitAny: TOutput defaults to any so untyped calls type-check (the engine cannot express the return type statically)
  trigger<TInput = unknown, TOutput = any>(request: TriggerRequest<TInput>): Promise<TOutput>

  /**
   * Observes a function: every event of `source` that passes its `filters`
   * invokes `target`, a registered function. Returns synchronously (also
   * before the connection is up); the observer is replayed with the same id
   * on reconnect. Errors show up as state `error` and reject `ready()`;
   * against an engine without observe that is `UNSUPPORTED`.
   *
   * @param source - What is observed: `function_id`, `args` (owner-delivered functions only), `filters`, `on`, `capture`, `deliver`.
   * @param target - The function to invoke.
   * @param options - `id`, `max_fires`, `expires_in_ms`, `max_in_flight`, `metadata`.
   *
   * @example
   * ```typescript
   * worker.registerFunction('ui::order-shipped', async ({ input }: Observed) => toast(input))
   *
   * const shipped = worker.observe(
   *   { function_id: 'orders::ship', filters: { 'input.customer_id': { $eq: customerId } } },
   *   'ui::order-shipped',
   * )
   * await shipped.ready()
   * ```
   */
  observe(source: ObserveSource, target: Target, options?: ObserverOptions): ObserverHandle

  /**
   * The invocation the calling code runs in (remote or local handler), or
   * `undefined` outside any handler.
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
   * invocation is running, and it is an observe activation); otherwise none.
   * They never get an invocation's metadata: `obs.emit` / `obs.call` filters
   * on `metadata.*` see it only inside a real scope. An invocation that never
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
   *   return worker.withInvocation(inv, () => worker.trigger({ function_id: 'orders::create', payload: quote }))
   * })
   * ```
   */
  currentInvocation(): InvocationContext | undefined

  /**
   * Runs `fn` inside `invocation` (what {@link ISdk.currentInvocation}
   * returned), and returns what `fn` returns. Calls `fn` makes before its own
   * first `await` (`trigger`, `obs.emit`, `obs.call`, `currentInvocation`)
   * belong to that invocation: they carry its caller, `observe_depth` and
   * `traceparent`. When `fn` returns a promise, the invocation counts as
   * running until it settles, so `fn`'s code after an `await` gets the
   * fallback below. `undefined` runs `fn` outside any invocation.
   *
   * Code after an `await`, outside `withInvocation`, runs outside any
   * invocation, but its calls are not treated as new chains: they carry the
   * `observe_depth` of the deepest invocation running on this page, so the
   * engine's loop guard (`max_observe_depth`) still stops an observer cycle
   * whose target awaits before calling back. With several invocations
   * running this over-counts, and a legitimate chain may stop early. They
   * carry a `caller` and `traceparent` only when that is unambiguous (one
   * invocation is running, and it is an observe activation); otherwise none.
   * They never get an invocation's metadata: `obs.emit` / `obs.call` filters
   * on `metadata.*` see it only inside a real scope. An invocation that never
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
   * Registers a new trigger type. A trigger type is a way to invoke a function when a certain event occurs.
   *
   * @deprecated Register a function with an observe descriptor instead:
   * `registerFunction(id, handler, { observe: { delivery: 'owner', onObserverAdded, onObserverRemoved } })`.
   * Logs a warning once per page. Migration guide: https://iii.dev/docs/upgrading/from-0-24-x
   * @param triggerType - The trigger type to register
   * @param handler - The handler for the trigger type
   * @returns A trigger type object that can be used to unregister the trigger type
   *
   * @example
   * ```typescript
   * type CronConfig = { expression: string }
   *
   * worker.registerTriggerType<CronConfig>(
   *   { id: 'cron', description: 'Fires on a cron schedule' },
   *   {
   *     async registerTrigger({ id, function_id, config }) {
   *       startCronJob(id, config.expression, () =>
   *         worker.trigger({ function_id, payload: {} }),
   *       )
   *     },
   *     async unregisterTrigger({ id }) {
   *       stopCronJob(id)
   *     },
   *   },
   * )
   * ```
   */
  registerTriggerType<TConfig>(
    triggerType: RegisterTriggerTypeInput,
    handler: TriggerHandler<TConfig>,
  ): TriggerTypeRef<TConfig>

  /**
   * Unregisters a trigger type.
   *
   * @deprecated Trigger types are replaced by functions with an observe descriptor.
   * @param triggerType - The trigger type to unregister
   *
   * @example
   * ```typescript
   * worker.unregisterTriggerType({ id: 'cron', description: 'Fires on a cron schedule' })
   * ```
   */
  unregisterTriggerType(triggerType: RegisterTriggerTypeInput): void

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
 * Handle returned by {@link ISdk.registerTrigger}. Use `unregister()` to
 * remove the trigger from the engine.
 *
 * @deprecated `registerTrigger` is replaced by `iii.observe`, which returns an {@link ObserverHandle}.
 */
export type Trigger = {
  /** Removes this trigger from the engine. */
  unregister(): void
}

/**
 * Handle returned by {@link ISdk.registerFunction}. Contains the function's
 * `id` and an `unregister()` method.
 */
export type FunctionRef = {
  /** The unique function identifier. */
  id: string
  /** Removes this function from the engine. */
  unregister: () => void
  /**
   * Whether any observer is active on this function (one load, for the emit
   * fast path: skip building an event nobody receives).
   */
  isObserved: () => boolean
  /** The active observers, as this owner sees them. */
  observers: () => readonly Observer[]
}

/**
 * Typed handle returned by {@link ISdk.registerTriggerType}.
 *
 * @deprecated Trigger types are replaced by functions with an observe
 * descriptor, and these helpers by `iii.observe`. Migration guide:
 * https://iii.dev/docs/upgrading/from-0-24-x
 *
 * Provides convenience methods to register triggers and functions scoped
 * to this trigger type, so callers don't need to repeat the `type` field.
 *
 * @typeParam TConfig - Trigger-specific configuration type.
 *
 * @example
 * ```typescript
 * type CronConfig = { expression: string }
 *
 * const cron = worker.registerTriggerType<CronConfig>(
 *   { id: 'cron', description: 'Fires on a cron schedule' },
 *   cronHandler,
 * )
 *
 * // Register a trigger; type is inferred as CronConfig
 * cron.registerTrigger('my::fn', { expression: '0 *\/5 * * * * *' })
 *
 * // Register a function and bind a trigger in one call
 * cron.registerFunction(
 *   'my::fn',
 *   async (data) => { return { ok: true } },
 *   { expression: '0 *\/5 * * * * *' },
 * )
 * ```
 */
export type TriggerTypeRef<TConfig = unknown> = {
  /** The trigger type identifier. */
  id: string
  /**
   * Register a trigger bound to this trigger type.
   *
   * @param functionId - The function to invoke when the trigger fires.
   * @param config - Trigger-specific configuration.
   * @returns A {@link Trigger} handle with an `unregister()` method.
   */
  registerTrigger(functionId: string, config: TConfig): Trigger
  /**
   * Register a function and immediately bind it to this trigger type.
   *
   * @param functionId - Unique function identifier.
   * @param handler - Local function handler.
   * @param config - Trigger-specific configuration.
   * @returns A {@link FunctionRef} handle.
   */
  registerFunction(functionId: string, handler: RemoteFunctionHandler, config: TConfig): FunctionRef
  /**
   * Unregister this trigger type from the engine.
   */
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
 * Incoming HTTP request received by a function registered with an HTTP trigger.
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
 * Structured API response returned from HTTP function handlers.
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
