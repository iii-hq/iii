export enum MessageType {
  RegisterFunction = 'registerfunction',
  RegisterService = 'registerservice',
  InvokeFunction = 'invokefunction',
  InvocationResult = 'invocationresult',
  RegisterTrigger = 'registertrigger',
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
  data?: any
  error?: any
}

export type BridgeMessage =
  | RegisterFunctionMessage
  | InvokeFunctionMessage
  | InvocationResultMessage
  | RegisterServiceMessage
  | RegisterTriggerMessage
