export enum MessageType {
  RegisterFunction = 'registerfunction',
  RegisterService = 'registerservice',
  InvokeFunction = 'invokefunction',
  InvocationResult = 'invocationresult',
  RegisterTriggerType = 'registertriggertype',
  RegisterTrigger = 'registertrigger',
  UnregisterTrigger = 'unregistertrigger',
  UnregisterTriggerType = 'unregistertriggertype',
  TriggerRegistrationResult = 'triggerregistrationresult',
}

export type RegisterTriggerTypeMessage = {
  type: MessageType.RegisterTriggerType
  id: string
  description: string
}

export type UnregisterTriggerTypeMessage = {
  type: MessageType.UnregisterTriggerType
  id: string
}

export type UnregisterTriggerMessage = {
  type: MessageType.UnregisterTrigger
  id: string
}

export type TriggerRegistrationResultMessage = {
  type: MessageType.TriggerRegistrationResult
  id: string
  triggerType: string
  functionPath: string
  result?: any
  error?: any
}

export type RegisterTriggerMessage = {
  type: MessageType.RegisterTrigger

  id: string
  triggerType: string // 'cron', 'event', 'http'
  /**
   * Entine path for the function, including the service and function name
   * Example: software.engineering.code.rust
   * Where software, engineering, and code are the service ids
   */
  functionPath: string
  config: any
}

export type RegisterServiceMessage = {
  type: MessageType.RegisterService
  id: string
  description?: string
  parentServiceId?: string
}

export type RegisterFunctionFormat = {
  name: string
  description?: string
  type: 'string' | 'number' | 'boolean' | 'object' | 'array' | 'null' | 'map'
  body?: RegisterFunctionFormat[]
  items?: RegisterFunctionFormat
  required?: boolean
}

export type RegisterFunctionMessage = {
  type: MessageType.RegisterFunction
  functionPath: string
  description?: string
  requestFormat?: RegisterFunctionFormat
  responseFormat?: RegisterFunctionFormat
  functionKind: 'http'| 'sync' | 'streaming'
}

export type InvokeFunctionMessage = {
  type: MessageType.InvokeFunction
  /**
   * This is optional for async invocations
   */
  invocationId?: string
  functionPath: string
  data: any
}

export type InvocationResultMessage = {
  type: MessageType.InvocationResult
  invocationId: string
  functionPath: string
  result?: any
  error?: any
}

export type BridgeMessage =
  | RegisterFunctionMessage
  | InvokeFunctionMessage
  | InvocationResultMessage
  | RegisterServiceMessage
  | RegisterTriggerMessage
  | RegisterTriggerTypeMessage
  | UnregisterTriggerMessage
  | UnregisterTriggerTypeMessage
  | TriggerRegistrationResultMessage
