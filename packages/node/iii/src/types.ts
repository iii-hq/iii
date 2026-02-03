import type {
  FunctionInfo,
  RegisterFunctionMessage,
  RegisterServiceMessage,
  RegisterTriggerMessage,
  RegisterTriggerTypeMessage,
} from './bridge-types'
import type { TriggerHandler } from './triggers'
import type { IStream } from './streams'

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

/** Severity levels for log filtering */
export type LogSeverityLevel = 'trace' | 'debug' | 'info' | 'warn' | 'error' | 'fatal' | 'all'

/** Optional configuration for onLog */
export type LogConfig = {
  /** Minimum severity level to receive (default: 'all') */
  level?: LogSeverityLevel
}

/** Callback type for log events */
export type LogCallback = (log: OtelLogEvent) => void
export type Invocation<TOutput = any> = { resolve: (data: TOutput) => void; reject: (error: any) => void }

/** Internal handler type that includes traceparent and baggage for distributed tracing */
export type InternalFunctionHandler<TInput = any, TOutput = any> = (
  data: TInput,
  traceparent?: string,
  baggage?: string,
) => Promise<TOutput>

export type RemoteFunctionData = {
  message: RegisterFunctionMessage
  handler: InternalFunctionHandler
}

export type RemoteServiceFunctionData = {
  message: Omit<RegisterFunctionMessage, 'serviceId'>
  handler: RemoteFunctionHandler
}

export type RemoteTriggerTypeData = {
  message: RegisterTriggerTypeMessage
  handler: TriggerHandler<any>
}

export type RegisterTriggerInput = Omit<RegisterTriggerMessage, 'type' | 'id'>
export type RegisterServiceInput = Omit<RegisterServiceMessage, 'type'>
export type RegisterFunctionInput = Omit<RegisterFunctionMessage, 'type'>
export type RegisterTriggerTypeInput = Omit<RegisterTriggerTypeMessage, 'type'>
export type FunctionsAvailableCallback = (functions: FunctionInfo[]) => void

export interface BridgeClient {
  /**
   * Registers a new trigger. A trigger is a way to invoke a function when a certain event occurs.
   * @param trigger - The trigger to register
   * @returns A trigger object that can be used to unregister the trigger
   */
  registerTrigger(trigger: RegisterTriggerInput): Trigger

  /**
   * Registers a new service. A service is a collection of functions that are related to each other.
   * @param service - The service to register
   * @returns A service object that can be used to unregister the service
   */
  registerService(service: RegisterServiceInput): void

  /**
   * Registers a new function. A function is a unit of work that can be invoked by other services.
   * @param func - The function to register
   * @param handler - The handler for the function
   * @returns A function object that can be used to invoke the function
   */
  registerFunction(func: RegisterFunctionInput, handler: RemoteFunctionHandler): void

  /**
   * Invokes a function.
   * @param function_path - The path to the function
   * @param data - The data to pass to the function
   * @returns The result of the function
   */
  invokeFunction<TInput, TOutput>(function_path: string, data: TInput): Promise<TOutput>

  /**
   * Invokes a function asynchronously.
   * @param function_path - The path to the function
   * @param data - The data to pass to the function
   */
  invokeFunctionAsync<TInput>(function_path: string, data: TInput): void

  /**
   * Registers a new trigger type. A trigger type is a way to invoke a function when a certain event occurs.
   * @param triggerType - The trigger type to register
   * @param handler - The handler for the trigger type
   * @returns A trigger type object that can be used to unregister the trigger type
   */
  registerTriggerType<TConfig>(triggerType: RegisterTriggerTypeInput, handler: TriggerHandler<TConfig>): void

  /**
   * Unregisters a trigger type.
   * @param triggerType - The trigger type to unregister
   */
  unregisterTriggerType(triggerType: RegisterTriggerTypeInput): void

  /**
   * Registers a callback for a specific event.
   * @param event - The event to register the callback for
   * @param callback - The callback to register
   */
  on(event: string, callback: (arg?: unknown) => void): void

  /**
   * Creates a new stream implementation.
   *
   * This overrides the default stream implementation.
   *
   * @param streamName - The name of the stream
   * @param stream - The stream implementation
   */
  createStream<TData>(streamName: string, stream: IStream<TData>): void

  /**
   * Registers a callback to receive the current functions list
   * when the engine announces changes.
   */
  onFunctionsAvailable(callback: FunctionsAvailableCallback): () => void

  /**
   * Registers a callback to receive OTEL log events from the engine.
   * @param callback - The callback to invoke when a log event is received
   * @param config - Optional configuration for filtering logs by severity level
   * @returns A function to unregister the callback
   */
  onLog(callback: LogCallback, config?: LogConfig): () => void
}

export type Trigger = {
  unregister(): void
}

export type ApiRequest<TBody = unknown> = {
  path_params: Record<string, string>
  query_params: Record<string, string | string[]>
  body: TBody
  headers: Record<string, string | string[]>
  method: string
}

export type ApiResponse<TStatus extends number = number, TBody = string | Buffer | Record<string, unknown>> = {
  status_code: TStatus
  headers?: Record<string, string>
  body: TBody
}
