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
  FunctionsAvailable = 'functionsavailable',
  ListFunctions = 'listfunctions',
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
  trigger_type: string
  function_path: string
  result?: any
  error?: any
}

export type TriggerConfig = {
  trigger_type: string
  config: any
}

export type RegisterTriggerMessage = {
  type: MessageType.RegisterTrigger
  id: string
  /**
   * Engine path for the function, including the service and function name
   * Example: software.engineering.code.rust
   * Where software, engineering, and code are the service ids
   */
  function_path: string
  triggers: TriggerConfig[]
}

export type RegisterServiceMessage = {
  type: MessageType.RegisterService
  id: string
  description?: string
  parent_service_id?: string
}

export type RegisterFunctionFormat = {
  name: string
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
  body?: RegisterFunctionFormat[]
  /**
   * The items of the parameter
   */
  items?: RegisterFunctionFormat
  /**
   * Whether the parameter is required
   */
  required?: boolean
}

export type RegisterFunctionMessage = {
  type: MessageType.RegisterFunction
  /**
   * The path of the function
   */
  function_path: string
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
}

export type InvokeFunctionMessage = {
  type: MessageType.InvokeFunction
  /**
   * This is optional for async invocations
   */
  invocation_id?: string
  /**
   * The path of the function
   */
  function_path: string
  /**
   * The data to pass to the function
   */
  data: any
}

export type InvocationResultMessage = {
  type: MessageType.InvocationResult
  /**
   * The id of the invocation
   */
  invocation_id: string
  /**
   * The path of the function
   */
  function_path: string
  result?: any
  error?: any
}

export type FunctionMessage = {
  functionPath: string
  description?: string
  requestFormat?: RegisterFunctionFormat
  responseFormat?: RegisterFunctionFormat
  metadata?: Record<string, unknown>
}

export type FunctionsAvailableMessage = {
  type: MessageType.FunctionsAvailable
  functions: FunctionMessage[]
}

export type ListFunctionsMessage = {
  type: MessageType.ListFunctions
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
  | FunctionsAvailableMessage
  | ListFunctionsMessage
