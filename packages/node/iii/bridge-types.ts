export enum MessageType {
  RegisterFunction = 'register-function',
  RegisterService = 'register-service',
  InvokeFunction = 'invoke-function',
  InvocationResult = 'invocation-result',
  RegisterTrigger = 'register-trigger',
}

export type RegisterTriggerMessage = {
  id: string
  type: string
  /**
   * Entine path for the function, including the service and function name
   * Example: software.engineering.code.rust
   * Where software, engineering, and code are the service ids
   */
  functionPath: string
  config: any
}

export type RegisterServiceMessage = {
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
  functionPath: string
  description?: string
  requestFormat?: RegisterFunctionFormat
  responseFormat?: RegisterFunctionFormat
}

export type InvokeFunctionMessage = {
  /**
   * This is optional for async invocations
   */
  invocationId?: string
  functionPath: string
  data: any
}

export type InvocationResultMessage = {
  invocationId: string
  functionPath: string
  data?: any
  error?: any
}

export type BridgeMessageData =
  | RegisterFunctionMessage
  | InvokeFunctionMessage
  | InvocationResultMessage
  | RegisterServiceMessage
  | RegisterTriggerMessage

export type BridgeMessage = {
  type: MessageType
  message: BridgeMessageData
}
