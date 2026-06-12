/** @deprecated Import channel symbols from `iii-sdk/channel`. */
export { ChannelReader, ChannelWriter } from './channel'
/** @deprecated Import `Channel` / `StreamChannelRef` from `iii-sdk/channel`. */
export type { Channel, StreamChannelRef } from './channel'

export { InvocationError, type InvocationErrorInit } from './errors'
/** @deprecated Renamed; import `InvocationError` / `InvocationErrorInit` from `iii-sdk/errors`. */
export { IIIInvocationError, type IIIInvocationErrorInit } from './errors'

export { type InitOptions, registerWorker, type TelemetryOptions, TriggerAction } from './iii'

export { EngineFunctions, EngineTriggers } from './iii-constants'

export type {
  MessageType,
  MiddlewareFunctionInput,
  RegisterFunctionMessage,
  RegisterTriggerMessage,
  RegisterTriggerTypeMessage,
  TriggerRequest,
} from './iii-types'

/** @deprecated Import trigger types from `iii-sdk/trigger`. */
export type { Trigger, TriggerConfig, TriggerHandler } from './trigger'

export type {
  IIIClient,
  RegisterFunctionInput,
  RegisterFunctionOptions,
  RegisterTriggerInput,
  RegisterTriggerTypeInput,
  RemoteFunctionHandler,
  StreamRequest,
  StreamResponse,
} from './types'

/** @deprecated Renamed to `IIIClient`. */
export type { ISdk } from './types'

/** @deprecated Import runtime types from `iii-sdk/runtime`. */
export type { FunctionRef, TriggerTypeRef } from './runtime'
