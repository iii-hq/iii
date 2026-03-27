export enum MessageType {
  RegisterFunction = 'registerfunction',
  UnregisterFunction = 'unregisterfunction',
  RegisterService = 'registerservice',
  InvokeFunction = 'invokefunction',
  InvocationResult = 'invocationresult',
  RegisterTriggerType = 'registertriggertype',
  RegisterTrigger = 'registertrigger',
  UnregisterTrigger = 'unregistertrigger',
  UnregisterTriggerType = 'unregistertriggertype',
  TriggerRegistrationResult = 'triggerregistrationresult',
  WorkerRegistered = 'workerregistered',
}

export type RegisterTriggerTypeMessage = {
  message_type: MessageType.RegisterTriggerType
  id: string
  description: string
}

export type UnregisterTriggerTypeMessage = {
  message_type: MessageType.UnregisterTriggerType
  id: string
}

export type UnregisterTriggerMessage = {
  message_type: MessageType.UnregisterTrigger
  id: string
  type?: string
}

export type TriggerRegistrationResultMessage = {
  message_type: MessageType.TriggerRegistrationResult
  id: string
  type: string
  function_id: string
  result?: unknown
  error?: unknown
}

export type RegisterTriggerMessage = {
  message_type: MessageType.RegisterTrigger
  id: string
  type: string
  function_id: string
  config: unknown
}

export type RegisterServiceMessage = {
  message_type: MessageType.RegisterService
  id: string
  name?: string
  description?: string
  parent_service_id?: string
}

/**
 * Authentication configuration for HTTP-invoked functions.
 *
 * - `hmac` -- HMAC signature verification using a shared secret.
 * - `bearer` -- Bearer token authentication.
 * - `api_key` -- API key sent via a custom header.
 */
export type HttpAuthConfig =
  | { type: 'hmac'; secret_key: string }
  | { type: 'bearer'; token_key: string }
  | { type: 'api_key'; header: string; value_key: string }

/**
 * Configuration for registering an HTTP-invoked function (Lambda, Cloudflare
 * Workers, etc.) instead of a local handler.
 */
export type HttpInvocationConfig = {
  /** URL to invoke. */
  url: string
  /** HTTP method. Defaults to `POST`. */
  method?: 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE'
  /** Timeout in milliseconds. */
  timeout_ms?: number
  /** Custom headers to send with the request. */
  headers?: Record<string, string>
  /** Authentication configuration. */
  auth?: HttpAuthConfig
}

export type RegisterFunctionFormat = {
  /**
   * The name of the parameter
   */
  name?: string
  /**
   * The description of the parameter
   */
  description?: string
  /**
   * The type of the parameter
   */
  type: 'string' | 'number' | 'boolean' | 'object' | 'array' | 'null' | 'map'
  /**
   * The body of the parameter
   */
  properties?: Record<string, RegisterFunctionFormat>
  /**
   * The items of the parameter
   */
  items?: RegisterFunctionFormat
  /**
   * Whether the parameter is required
   */
  required?: string[]
}

export type RegisterFunctionMessage = {
  message_type: MessageType.RegisterFunction
  /**
   * The path of the function (use :: for namespacing, e.g. external::my_lambda)
   */
  id: string
  /**
   * The description of the function
   */
  description?: string
  /**
   * The request format of the function
   */
  request_format?: RegisterFunctionFormat
  /**
   * The response format of the function
   */
  response_format?: RegisterFunctionFormat
  metadata?: Record<string, unknown>
  /**
   * HTTP invocation config for external HTTP functions (Lambda, Cloudflare Workers, etc.)
   */
  invocation?: HttpInvocationConfig
}

/**
 * Routing action for {@link TriggerRequest}. Determines how the engine
 * handles the invocation.
 *
 * - `enqueue` -- Routes through a named queue for async processing.
 * - `void` -- Fire-and-forget, no response.
 */
export type TriggerAction = { type: 'enqueue'; queue: string } | { type: 'void' }

/**
 * Result returned when a function is invoked with `TriggerAction.Enqueue`.
 */
export type EnqueueResult = {
  /** Unique receipt ID for the enqueued message. */
  messageReceiptId: string
}

/**
 * Request object passed to {@link ISdk.trigger}.
 *
 * @typeParam TInput - Type of the payload.
 */
export type TriggerRequest<TInput = unknown> = {
  /** ID of the function to invoke. */
  function_id: string
  /** Payload to pass to the function. */
  payload: TInput
  /** Routing action. Omit for synchronous request/response. */
  action?: TriggerAction
  /** Override the default invocation timeout in milliseconds. */
  timeoutMs?: number
}

export type InvokeFunctionMessage = {
  message_type: MessageType.InvokeFunction
  /**
   * This is optional for async invocations
   */
  invocation_id?: string
  /**
   * The path of the function
   */
  function_id: string
  /**
   * The data to pass to the function
   */
  data: unknown
  /**
   * W3C trace-context traceparent header for distributed tracing
   */
  traceparent?: string
  /**
   * W3C baggage header for cross-cutting context propagation
   */
  baggage?: string
  /**
   * Trigger action for queue routing or fire-and-forget
   */
  action?: TriggerAction
}

export type InvocationResultMessage = {
  message_type: MessageType.InvocationResult
  /**
   * The id of the invocation
   */
  invocation_id: string
  /**
   * The path of the function
   */
  function_id: string
  result?: unknown
  error?: unknown
  /**
   * W3C trace-context traceparent header for distributed tracing
   */
  traceparent?: string
  /**
   * W3C baggage header for cross-cutting context propagation
   */
  baggage?: string
}

/**
 * Metadata about a registered function, returned by `ISdk.listFunctions`.
 */
export type FunctionInfo = {
  /** Unique function identifier. */
  function_id: string
  /** Human-readable description. */
  description?: string
  /** Schema describing expected request format. */
  request_format?: RegisterFunctionFormat
  /** Schema describing expected response format. */
  response_format?: RegisterFunctionFormat
  /** Arbitrary metadata attached to the function. */
  metadata?: Record<string, unknown>
}

/**
 * Information about a registered trigger.
 */
export type TriggerInfo = {
  /** Unique trigger identifier. */
  id: string
  /** Type of the trigger (e.g. `http`, `cron`, `queue`). */
  trigger_type: string
  /** ID of the function this trigger is bound to. */
  function_id: string
  /** Trigger-specific configuration. */
  config?: unknown
}

/**
 * Information about a registered trigger type, returned by `ISdk.listTriggerTypes`.
 */
export type TriggerTypeInfo = {
  /** Trigger type identifier (e.g. `http`, `cron`, `queue`). */
  id: string
  /** Human-readable description of the trigger type. */
  description: string
  /** JSON Schema for the trigger configuration. */
  trigger_request_format?: unknown
  /** JSON Schema for the call request payload. */
  call_request_format?: unknown
}

/** Worker connection status. */
export type WorkerStatus = 'connected' | 'available' | 'busy' | 'disconnected'

/**
 * Metadata about a connected worker, returned by `ISdk.listWorkers`.
 */
export type WorkerInfo = {
  /** Unique worker identifier assigned by the engine. */
  id: string
  /** Display name of the worker. */
  name?: string
  /** Runtime environment (e.g. `node`, `python`, `rust`). */
  runtime?: string
  /** SDK version. */
  version?: string
  /** Operating system info. */
  os?: string
  /** IP address of the worker. */
  ip_address?: string
  /** Current connection status. */
  status: WorkerStatus
  /** Timestamp (ms since epoch) when the worker connected. */
  connected_at_ms: number
  /** Number of functions registered by this worker. */
  function_count: number
  /** List of function IDs registered by this worker. */
  functions: string[]
  /** Number of currently active invocations. */
  active_invocations: number
}

export type WorkerRegisteredMessage = {
  message_type: MessageType.WorkerRegistered
  worker_id: string
}

export type UnregisterFunctionMessage = {
  message_type: MessageType.UnregisterFunction
  id: string
}

/**
 * Serializable reference to one end of a streaming channel. Can be included
 * in invocation payloads to pass channel endpoints between workers.
 */
export type StreamChannelRef = {
  /** Unique channel identifier. */
  channel_id: string
  /** Access key for authentication. */
  access_key: string
  /** Whether this ref is for reading or writing. */
  direction: 'read' | 'write'
}

export type IIIMessage =
  | RegisterFunctionMessage
  | UnregisterFunctionMessage
  | InvokeFunctionMessage
  | InvocationResultMessage
  | RegisterServiceMessage
  | RegisterTriggerMessage
  | RegisterTriggerTypeMessage
  | UnregisterTriggerMessage
  | UnregisterTriggerTypeMessage
  | TriggerRegistrationResultMessage
  | WorkerRegisteredMessage
