import type { ChannelReader, ChannelWriter } from './channels'
import type {
  RegisterFunctionMessage,
  RegisterServiceMessage,
  RegisterTriggerMessage,
  RegisterTriggerTypeMessage,
  StreamChannelRef,
  TriggerRequest,
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
export type RegisterFunctionOptions = Omit<RegisterFunctionMessage, 'message_type' | 'id'>
export type RegisterTriggerTypeInput = Omit<RegisterTriggerTypeMessage, 'message_type'>

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
   *   config: { expression: '0 *\/5 * * * * *' },
   * })
   *
   * // Later, remove the trigger
   * trigger.unregister()
   * ```
   */
  registerTrigger(trigger: RegisterTriggerInput): Trigger

  /**
   * Registers a new service.
   * @param message - The service to register
   */
  registerService(message: RegisterServiceInput): void

  /**
   * Registers a new function with a local handler or an HTTP invocation config.
   * @param functionId - Unique function identifier
   * @param handler - Async handler for local execution, or an HTTP invocation config for external functions (Lambda, Cloudflare Workers, etc.)
   * @param options - Optional function registration options (description, request/response formats, metadata)
   * @returns A handle that can be used to unregister the function
   *
   * @example
   * ```typescript
   * // Local handler
   * const ref = iii.registerFunction(
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
   * Registers a new trigger type. A trigger type is a way to invoke a function when a certain event occurs.
   * @param triggerType - The trigger type to register
   * @param handler - The handler for the trigger type
   * @returns A trigger type object that can be used to unregister the trigger type
   *
   * @example
   * ```typescript
   * type CronConfig = { expression: string }
   *
   * iii.registerTriggerType<CronConfig>(
   *   { id: 'cron', description: 'Fires on a cron schedule' },
   *   {
   *     async registerTrigger({ id, function_id, config }) {
   *       startCronJob(id, config.expression, () =>
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
  registerTriggerType<TConfig>(
    triggerType: RegisterTriggerTypeInput,
    handler: TriggerHandler<TConfig>,
  ): TriggerTypeRef<TConfig>

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
   * iii.createStream('sessions', {
   *   async get({ group_id, item_id }) { return null },
   *   async set({ group_id, item_id, data }) { return { new_value: data } },
   *   async delete({ group_id, item_id }) { return {} },
   *   async list({ group_id }) { return [] },
   *   async listGroups() { return [] },
   *   async update() { return null },
   * })
   * ```
   */
  createStream<TData>(streamName: string, stream: IStream<TData>): void

  /**
   * Gracefully shutdown the iii, cleaning up all resources.
   *
   * @example
   * ```typescript
   * await iii.shutdown()
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
 * type CronConfig = { expression: string }
 *
 * const cron = iii.registerTriggerType<CronConfig>(
 *   { id: 'cron', description: 'Fires on a cron schedule' },
 *   cronHandler,
 * )
 *
 * // Register a trigger -- type is inferred as CronConfig
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
