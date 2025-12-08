import {
  RegisterFunctionMessage,
  RegisterServiceMessage,
  RegisterTriggerMessage,
  RegisterTriggerTypeMessage,
} from './bridge-types'
import { TriggerHandler } from './triggers'

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

export type RegisterServiceInput = Omit<RegisterServiceMessage, 'functions'>

export interface BridgeClient {
  registerTrigger(trigger: Omit<RegisterTriggerMessage, 'type' | 'id'>): Trigger
  registerService(service: Omit<RegisterServiceMessage, 'type'>): void
  registerFunction(func: Omit<RegisterFunctionMessage, 'type'>, handler: RemoteFunctionHandler): void
  invokeFunction<TInput, TOutput>(functionId: string, data: TInput): Promise<TOutput>
  invokeFunctionAsync<TInput>(functionId: string, data: TInput): void

  registerTriggerType<TConfig>(
    triggerType: Omit<RegisterTriggerTypeMessage, 'type'>,
    handler: TriggerHandler<TConfig>,
  ): void
  unregisterTriggerType(triggerType: Omit<RegisterTriggerTypeMessage, 'type'>): void

  // in the future we're going to have unregister functions, services, and triggers
  // so we can clean up the resources when the client is no longer needed
  // unregisterFunction(func: RegisterFunctionMessage): void
  // unregisterService(service: RegisterServiceInput): void
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
