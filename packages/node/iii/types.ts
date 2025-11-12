import { RegisterFunctionMessage, RegisterServiceMessage, RegisterTriggerMessage } from './bridge-types'

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

export type RegisterServiceInput = Omit<RegisterServiceMessage, 'functions'>

export interface BridgeClient {
  registerTrigger(trigger: Omit<RegisterTriggerMessage, 'type'>): void
  registerService(service: Omit<RegisterServiceMessage, 'type'>): void
  registerFunction(func: Omit<RegisterFunctionMessage, 'type'>, handler: RemoteFunctionHandler): void
  invokeFunction<TInput, TOutput>(functionId: string, data: TInput): Promise<TOutput>
  invokeFunctionAsync<TInput>(functionId: string, data: TInput): void

  // in the future we're going to have unregister functions, services, and triggers
  // so we can clean up the resources when the client is no longer needed
  // unregisterFunction(func: RegisterFunctionMessage): void
  // unregisterService(service: RegisterServiceInput): void
  // unregisterTrigger(trigger: RegisterTriggerMessage): void
}
