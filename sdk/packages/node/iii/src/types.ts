import type { ChannelReader, ChannelWriter } from './channels'
import type {
  FunctionInfo,
  HttpInvocationConfig,
  RegisterFunctionMessage,
  RegisterServiceMessage,
  RegisterTriggerMessage,
  RegisterTriggerTypeMessage,
  StreamChannelRef,
  TriggerInfo,
  TriggerRequest,
  TriggerTypeInfo,
} from './iii-types'
import type { IStream } from './stream'
import type { TriggerHandler } from './triggers'

/**
 * Async function handler for a registered function. Receives the invocation
 * payload and returns the result.
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
export type RemoteFunctionHandler<TInput = any, TOutput = any> = (data: TInput) => Promise<TOutput>

/** OTEL Log Event from the engine */
export type OtelLogEvent = {
  /** Timestamp in Unix nanoseconds */
  timestamp_unix_nano: number
  /** Observed timestamp in Unix nanoseconds */
  observed_timestamp_unix_nano: number
  /** OTEL severity number (1-24): TRACE=1-4, DEBUG=5-8, INFO=9-12, WARN=13-16, ERROR=17-20, FATAL=21-24 */
  severity_number: number
  /** Severity text (e.g., "INFO", "WARN", "ERROR") */
  severity_text: string
  /** Log message body */
  body: string
  /** Structured attributes */
  attributes: Record<string, unknown>
  /** Trace ID for correlation (if available) */
  trace_id?: string
  /** Span ID for correlation (if available) */
  span_id?: string
  /** Resource attributes from the emitting service */
  resource: Record<string, string>
  /** Service name that emitted the log */
  service_name: string
  /** Instrumentation scope name (if available) */
  instrumentation_scope_name?: string
  /** Instrumentation scope version (if available) */
  instrumentation_scope_version?: string
}

// biome-ignore lint/suspicious/noExplicitAny: generic default requires any for contravariant compatibility
export type Invocation<TOutput = any> = {
  resolve: (data: TOutput) => void
  // biome-ignore lint/suspicious/noExplicitAny: error can be any type
  reject: (error: any) => void
}

/** Internal handler type that includes traceparent and baggage for distributed tracing */
// biome-ignore lint/suspicious/noExplicitAny: generic defaults require any for contravariant compatibility
export type InternalFunctionHandler<TInput = any, TOutput = any> = (
  data: TInput,
  traceparent?: string,
  baggage?: string,
) => Promise<TOutput>

export type RemoteFunctionData = {
  message: RegisterFunctionMessage
  handler?: InternalFunctionHandler
}

export type RemoteServiceFunctionData = {
  message: Omit<RegisterFunctionMessage, 'serviceId'>
  handler: RemoteFunctionHandler
}

export type RemoteTriggerTypeData = {
  message: RegisterTriggerTypeMessage
  // biome-ignore lint/suspicious/noExplicitAny: handler accepts any trigger config type
  handler: TriggerHandler<any>
}

export type RegisterTriggerInput = Omit<RegisterTriggerMessage, 'message_type' | 'id'>
export type RegisterServiceInput = Omit<RegisterServiceMessage, 'message_type'>
export type RegisterFunctionInput = Omit<RegisterFunctionMessage, 'message_type'>
export type RegisterTriggerTypeInput = Omit<RegisterTriggerTypeMessage, 'message_type'>
export type FunctionsAvailableCallback = (functions: FunctionInfo[]) => void

export interface ISdk {
  /**
   * Registers a new trigger. A trigger is a way to invoke a function when a certain event occurs.
   * @param trigger - The trigger to register
   * @returns A trigger object that can be used to unregister the trigger
   *
   * @example
   * ```typescript
   * const trigger = iii.registerTrigger({
   *   type: 'cron',
   *   function_id: 'my-service::process-batch',
   *   config: { schedule: '*\/5 * * * *' },
   * })
   *
   * // Later, remove the trigger
   * trigger.unregister()
   * ```
   */
  registerTrigger(trigger: RegisterTriggerInput): Trigger

  /**
   * Registers a new function with a local handler.
   * @param func - The function to register
   * @param handler - The handler for local execution
   * @returns A handle that can be used to unregister the function
   *
   * @example
   * ```typescript
   * const ref = iii.registerFunction(
   *   { id: 'greet', description: 'Returns a greeting' },
   *   async (data: { name: string }) => ({ message: `Hello, ${data.name}!` }),
   * )
   *
   * // Later, remove the function
   * ref.unregister()
   * ```
   */
  /**
   * Registers a new service.
   * @param message - The service to register
   */
  registerService(message: RegisterServiceInput): void

  registerFunction(func: RegisterFunctionInput, handler: RemoteFunctionHandler): FunctionRef
  /**
   * Registers a new function with an HTTP invocation config (Lambda, Cloudflare Workers, etc.).
   * @param func - The function to register
   * @param invocation - HTTP invocation config
   * @returns A handle that can be used to unregister the function
   *
   * @example
   * ```typescript
   * const ref = iii.registerFunction(
   *   { id: 'external::my-lambda', description: 'Proxied Lambda function' },
   *   {
   *     url: 'https://abc123.lambda-url.us-east-1.on.aws',
   *     method: 'POST',
   *     timeout_ms: 30_000,
   *     auth: { type: 'bearer', token_key: 'LAMBDA_AUTH_TOKEN' },
   *   },
   * )
   * ```
   */
  registerFunction(func: RegisterFunctionInput, invocation: HttpInvocationConfig): FunctionRef

  /**
   * Invokes a function using a request object.
   *
   * @param request - The trigger request containing function_id, payload, and optional action/timeout
   * @returns The result of the function
   *
   * @example
   * ```typescript
   * // Synchronous invocation
   * const result = await iii.trigger<{ name: string }, { message: string }>({
   *   function_id: 'greet',
   *   payload: { name: 'World' },
   *   timeoutMs: 5000,
   * })
   * console.log(result.message) // "Hello, World!"
   *
   * // Fire-and-forget
   * await iii.trigger({
   *   function_id: 'send-email',
   *   payload: { to: 'user@example.com' },
   *   action: TriggerAction.Void(),
   * })
   *
   * // Enqueue for async processing
   * const receipt = await iii.trigger({
   *   function_id: 'process-order',
   *   payload: { orderId: '123' },
   *   action: TriggerAction.Enqueue({ queue: 'orders' }),
   * })
   * ```
   */
  trigger<TInput, TOutput>(request: TriggerRequest<TInput>): Promise<TOutput>

  /**
   * Lists all registered functions.
   *
   * @example
   * ```typescript
   * const functions = await iii.listFunctions()
   * for (const fn of functions) {
   *   console.log(`${fn.function_id}: ${fn.description}`)
   * }
   * ```
   */
  listFunctions(): Promise<FunctionInfo[]>

  /**
   * Lists all registered triggers.
   * @param includeInternal - Whether to include internal triggers (default: false)
   */
  listTriggers(includeInternal?: boolean): Promise<TriggerInfo[]>

  /**
   * Lists all trigger types registered with the engine.
   * @param includeInternal - Whether to include internal trigger types (default: false)
   *
   * @example
   * ```typescript
   * const triggerTypes = await iii.listTriggerTypes()
   * for (const tt of triggerTypes) {
   *   console.log(`${tt.id}: ${tt.description}`)
   * }
   * ```
   */
  listTriggerTypes(includeInternal?: boolean): Promise<TriggerTypeInfo[]>

  /**
   * Registers a new trigger type. A trigger type is a way to invoke a function when a certain event occurs.
   * @param triggerType - The trigger type to register
   * @param handler - The handler for the trigger type
   * @returns A trigger type object that can be used to unregister the trigger type
   *
   * @example
   * ```typescript
   * type CronConfig = { schedule: string }
   *
   * iii.registerTriggerType<CronConfig>(
   *   { id: 'cron', description: 'Fires on a cron schedule' },
   *   {
   *     async registerTrigger({ id, function_id, config }) {
   *       startCronJob(id, config.schedule, () =>
   *         iii.trigger({ function_id, payload: {} }),
   *       )
   *     },
   *     async unregisterTrigger({ id }) {
   *       stopCronJob(id)
   *     },
   *   },
   * )
   * ```
   */
  registerTriggerType<TConfig>(triggerType: RegisterTriggerTypeInput, handler: TriggerHandler<TConfig>): TriggerTypeRef<TConfig>

  /**
   * Unregisters a trigger type.
   * @param triggerType - The trigger type to unregister
   *
   * @example
   * ```typescript
   * iii.unregisterTriggerType({ id: 'cron', description: 'Fires on a cron schedule' })
   * ```
   */
  unregisterTriggerType(triggerType: RegisterTriggerTypeInput): void

  /**
   * Creates a streaming channel pair for worker-to-worker data transfer.
   * Returns a Channel with a local writer/reader and serializable refs that
   * can be passed as fields in the invocation data to other functions.
   *
   * @param bufferSize - Optional buffer size for the channel (default: 64)
   * @returns A Channel with writer, reader, and their serializable refs
   *
   * @example
   * ```typescript
   * const channel = await iii.createChannel()
   *
   * // Pass the writer ref to another function
   * await iii.trigger({
   *   function_id: 'stream-producer',
   *   payload: { outputChannel: channel.writerRef },
   * })
   *
   * // Read data locally
   * channel.reader.onMessage((msg) => {
   *   console.log('Received:', msg)
   * })
   * ```
   */
  createChannel(bufferSize?: number): Promise<Channel>

  /**
   * Creates a new stream implementation.
   *
   * This overrides the default stream implementation.
   *
   * @param streamName - The name of the stream
   * @param stream - The stream implementation
   *
   * @example
   * ```typescript
   * const redisStream: IStream<UserSession> = {
   *   async get({ group_id, item_id }) {
   *     return JSON.parse(await redis.get(`${group_id}:${item_id}`) ?? 'null')
   *   },
   *   async set({ group_id, item_id, data }) {
   *     const old = await this.get({ stream_name: 'sessions', group_id, item_id })
   *     await redis.set(`${group_id}:${item_id}`, JSON.stringify(data))
   *     return { old_value: old ?? undefined, new_value: data }
   *   },
   *   async delete({ group_id, item_id }) {
   *     const old = await this.get({ stream_name: 'sessions', group_id, item_id })
   *     await redis.del(`${group_id}:${item_id}`)
   *     return { old_value: old ?? undefined }
   *   },
   *   async list({ group_id }) { return [] },
   *   async listGroups() { return [] },
   *   async update({ group_id, item_id, ops }) { return { new_value: {} } },
   * }
   *
   * iii.createStream('sessions', redisStream)
   * ```
   */
  createStream<TData>(streamName: string, stream: IStream<TData>): void

  /**
   * Registers a callback to receive the current functions list
   * when the engine announces changes.
   *
   * @example
   * ```typescript
   * const unsubscribe = iii.onFunctionsAvailable((functions) => {
   *   console.log(`${functions.length} functions available:`)
   *   for (const fn of functions) {
   *     console.log(`  - ${fn.function_id}`)
   *   }
   * })
   *
   * // Later, stop listening
   * unsubscribe()
   * ```
   */
  onFunctionsAvailable(callback: FunctionsAvailableCallback): () => void

  /**
   * Gracefully shutdown the iii, cleaning up all resources.
   *
   * @example
   * ```typescript
   * process.on('SIGTERM', async () => {
   *   await iii.shutdown()
   *   process.exit(0)
   * })
   * ```
   */
  shutdown(): Promise<void>
}

/**
 * Handle returned by {@link ISdk.registerTrigger}. Use `unregister()` to
 * remove the trigger from the engine.
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
}

/**
 * Typed handle returned by {@link ISdk.registerTriggerType}.
 *
 * Provides convenience methods to register triggers and functions scoped
 * to this trigger type, so callers don't need to repeat the `type` field.
 *
 * @typeParam TConfig - Trigger-specific configuration type.
 *
 * @example
 * ```typescript
 * type CronConfig = { schedule: string }
 *
 * const cron = iii.registerTriggerType<CronConfig>(
 *   { id: 'cron', description: 'Fires on a cron schedule' },
 *   cronHandler,
 * )
 *
 * // Register a trigger — type is inferred as CronConfig
 * cron.registerTrigger('my-fn', { schedule: '* * * * *' })
 *
 * // Register a function and bind a trigger in one call
 * cron.registerFunction(
 *   { id: 'my-fn', description: 'Cron-triggered function' },
 *   async (data) => { return { ok: true } },
 *   { schedule: '* * * * *' },
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
   * @param func - Function registration input.
   * @param handler - Local function handler.
   * @param config - Trigger-specific configuration.
   * @returns A {@link FunctionRef} handle.
   */
  registerFunction(func: RegisterFunctionInput, handler: RemoteFunctionHandler, config: TConfig): FunctionRef
  /**
   * Unregister this trigger type from the engine.
   */
  unregister(): void
}

/**
 * A streaming channel pair for worker-to-worker data transfer. Created via
 * {@link ISdk.createChannel}.
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
 * Response object passed to HTTP function handlers. Use `status()` and
 * `headers()` to set response metadata, write to `stream` for streaming
 * responses, and call `close()` when done.
 */
export type HttpResponse = {
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
 * Incoming HTTP request received by a function registered with an HTTP trigger.
 *
 * @typeParam TBody - Type of the parsed request body.
 */
export type HttpRequest<TBody = unknown> = Omit<InternalHttpRequest<TBody>, 'response'>

/**
 * Alias for {@link HttpRequest}. Represents an incoming API request.
 *
 * @typeParam TBody - Type of the parsed request body.
 */
export type ApiRequest<TBody = unknown> = HttpRequest<TBody>

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
export type ApiResponse<TStatus extends number = number, TBody = string | Buffer | Record<string, unknown>> = {
  /** HTTP status code. */
  status_code: TStatus
  /** Response headers. */
  headers?: Record<string, string>
  /** Response body. */
  body?: TBody
}
