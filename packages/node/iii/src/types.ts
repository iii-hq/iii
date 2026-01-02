import type {
  RegisterFunctionMessage,
  RegisterServiceMessage,
  RegisterTriggerMessage,
  RegisterTriggerTypeMessage,
} from './bridge-types'
import type { TriggerHandler } from './triggers'

export type RemoteFunctionHandler<TInput = any, TOutput = any> = (data: TInput) => Promise<TOutput>
export type Invocation<TOutput = any> = { resolve: (data: TOutput) => void; reject: (error: any) => void }

export type RemoteFunctionData = {
  message: RegisterFunctionMessage
  handler: RemoteFunctionHandler
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
