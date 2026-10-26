import type { HttpInvocationConfig } from '@iii-dev/helpers/http'
import type { ChannelReader, ChannelWriter } from './channels'
import type { RegistrationRejectedError } from './errors'
import type { IIIConnectionState } from './iii-constants'
import type {
  JsonValue,
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
 * payload and an optional per-invocation `metadata` value, and returns the
 * result.
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
) => Promise<TOutput>

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

/**
 * Internal handler type. Wraps the user {@link RemoteFunctionHandler} and adds
 * the trace context (traceparent/baggage) threaded internally by the SDK.
 * `metadata` mirrors the user-facing argument; trace context stays internal
 * and is not surfaced to the user handler.
 */
// biome-ignore lint/suspicious/noExplicitAny: generic defaults require any for contravariant compatibility
export type InternalFunctionHandler<TInput = any, TOutput = any> = (
  data: TInput,
  metadata?: JsonValue,
  traceparent?: string,
  baggage?: string,
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
 * Options of {@link IIIClient.registerFunction}: description, request/response
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

export interface IIIClient {
  /**
   * Registers a new trigger. A trigger is a way to invoke a function when a certain event occurs.
   *
   * @deprecated Use {@link IIIClient.observe} instead: `iii.observe({ function_id, args, filters }, target, { metadata })`.
   * Logs a warning once per process. The http, cron, queue, state, stream, pubsub and database workers
   * still publish trigger types: keep using `registerTrigger` for them until they ship source functions.
   * Migration guide: https://iii.dev/docs/upgrading/from-0-24-x
   * <!-- docs:expand-params -->
   * @param trigger - The trigger to register.
   * @returns A trigger object that can be used to unregister the trigger.
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
   * Registers a new function with a local handler or an HTTP invocation config.
   * @param functionId - Unique identifier for the function.
   * @param handler - Async handler for local execution, or an HTTP invocation config for external functions (Lambda, Cloudflare Workers, etc.).
   * @param options - Optional function registration options (description, request/response formats, metadata).
   * @returns A handle that can be used to unregister the function.
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
  registerFunction(
    functionId: string,
    handler: RemoteFunctionHandler | HttpInvocationConfig,
    options?: RegisterFunctionOptions,
  ): FunctionRef

  /**
   * Invokes a function using a request object.
   * <!-- docs:expand-params -->
   *
   * @param request - The trigger request containing function_id, payload, and optional action/timeout.
   * @returns The result of the function.
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
   * on reconnect. Errors show up as state `error` and reject `ready()`.
   *
   * @param source - What is observed: `function_id`, `args` (owner-delivered functions only), `filters`, `on`, `capture`, `deliver`.
   * @param target - The function to invoke.
   * @param options - `id`, `max_fires`, `expires_in_ms`, `max_in_flight`, `metadata`.
   *
   * @example
   * ```typescript
   * // A regular function: large orders reach alerts::big-order
   * worker.observe(
   *   { function_id: 'orders::create', filters: { 'input.total': { $gte: 500 } } },
   *   'alerts::big-order',
   * )
   *
   * // An engine-hosted event source, delivered as the event itself
   * const workers = worker.observe(
   *   { function_id: 'engine::on-workers-changed', deliver: 'input' },
   *   'ops::on-worker-change',
   * )
   * await workers.ready()
   * ```
   */
  observe(source: ObserveSource, target: Target, options?: ObserverOptions): ObserverHandle

  /**
   * The invocation the calling code runs in (remote or local handler), or
   * `undefined` outside any handler.
   */
  currentInvocation(): InvocationContext | undefined

  /**
   * Registers a new trigger type. A trigger type is a way to invoke a function when a certain event occurs.
   *
   * @deprecated Register a function with an observe descriptor instead:
   * `registerFunction(id, handler, { observe: { delivery: 'owner', onObserverAdded, onObserverRemoved } })`.
   * Logs a warning once per process. Migration guide: https://iii.dev/docs/upgrading/from-0-24-x
   * @param triggerType - The trigger type to register.
   * @param handler - The handler for the trigger type.
   * @returns A trigger type object that can be used to unregister the trigger type.
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
   * @param triggerType - The trigger type to unregister.
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
 * Handle returned by {@link IIIClient.registerTrigger}. Use `unregister()` to
 * remove the trigger from the engine.
 *
 * @deprecated `registerTrigger` is replaced by `iii.observe`, which returns an {@link ObserverHandle}.
 */
export type Trigger = {
  /** Removes this trigger from the engine. */
  unregister(): void
}

/**
 * Handle returned by {@link IIIClient.registerFunction}. Contains the function's
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
 * Typed handle returned by {@link IIIClient.registerTriggerType}.
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
 * // Register a trigger, type is inferred as CronConfig
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
   * @param metadata - Optional arbitrary metadata attached to the trigger.
   * @returns A {@link Trigger} handle with an `unregister()` method.
   */
  registerTrigger(functionId: string, config: TConfig, metadata?: Record<string, unknown>): Trigger
  /**
   * Register a function and immediately bind it to this trigger type.
   *
   * @param functionId - Unique function identifier.
   * @param handler - Local function handler.
   * @param config - Trigger-specific configuration.
   * @param metadata - Optional arbitrary metadata attached to the trigger.
   * @returns A {@link FunctionRef} handle.
   */
  registerFunction(
    functionId: string,
    handler: RemoteFunctionHandler,
    config: TConfig,
    metadata?: Record<string, unknown>,
  ): FunctionRef
  /**
   * Unregister this trigger type from the engine.
   */
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
 * Incoming streaming request received by a function registered with a stream trigger.
 *
 * @typeParam TBody - Type of the parsed request body.
 */
export type StreamRequest<TBody = unknown> = Omit<InternalHttpRequest<TBody>, 'response'>
