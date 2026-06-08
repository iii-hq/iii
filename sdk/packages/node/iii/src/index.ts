/** @deprecated Import channel symbols from `iii-sdk/channel`. */
export { ChannelReader, ChannelWriter } from './channel'
/** @deprecated Import `Channel` / `StreamChannelRef` from `iii-sdk/channel`. */
export type { Channel, StreamChannelRef } from './channel'

export { IIIInvocationError, type IIIInvocationErrorInit } from './errors'

export { type InitOptions, registerWorker, TriggerAction } from './iii'

export { EngineFunctions, EngineTriggers } from './iii-constants'

export type {
  AuthInput,
  AuthResult,
  EnqueueResult,
  HttpAuthConfig,
  HttpInvocationConfig,
  MessageType,
  MiddlewareFunctionInput,
  OnFunctionRegistrationInput,
  OnFunctionRegistrationResult,
  OnTriggerRegistrationInput,
  OnTriggerRegistrationResult,
  OnTriggerTypeRegistrationInput,
  OnTriggerTypeRegistrationResult,
  RegisterFunctionMessage,
  RegisterTriggerMessage,
  RegisterTriggerTypeMessage,
  TriggerRequest,
} from './iii-types'

/** @deprecated Import trigger types from `iii-sdk/trigger`. */
export type { Trigger, TriggerConfig, TriggerHandler } from './trigger'

export type {
  ApiRequest,
  ApiResponse,
  FunctionRef,
  HttpRequest,
  HttpResponse,
  InternalHttpRequest,
  ISdk,
  RegisterFunctionInput,
  RegisterFunctionOptions,
  RegisterTriggerInput,
  RegisterTriggerTypeInput,
  RemoteFunctionHandler,
  TriggerTypeRef,
} from './types'

export { http } from './utils'
