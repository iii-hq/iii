/**
 * Helper free functions that operate on an {@link ISdk} instance.
 *
 * These were previously instance methods on the SDK. They take the iii
 * instance as the first argument so the public API surface of `ISdk` stays
 * focused on the core lifecycle and registration methods.
 */
import type { Channel, ISdk, RegisterTriggerTypeInput, TriggerTypeRef } from './types'
import type { IStream } from './stream'
import type { TriggerHandler } from './triggers'

export { ChannelDirection, ChannelItem } from './channels'
export { extractChannelRefs, isChannelRef } from './utils'

type IIIWithHelperShims = ISdk & {
  __helpers_create_channel(bufferSize?: number): Promise<Channel>
  __helpers_create_stream<T>(name: string, stream: IStream<T>): void
  __helpers_register_trigger_type<T>(
    triggerType: RegisterTriggerTypeInput,
    handler: TriggerHandler<T>,
  ): TriggerTypeRef<T>
  __helpers_unregister_trigger_type(id: string): void
}

/**
 * Create a streaming channel pair for worker-to-worker data transfer.
 *
 * Free-function form of the previous `ISdk.createChannel` instance method.
 */
export function createChannel(iii: ISdk, bufferSize?: number): Promise<Channel> {
  return (iii as IIIWithHelperShims).__helpers_create_channel(bufferSize)
}

/**
 * Register a custom stream implementation by wiring its 5 callable methods
 * to `stream::get/set/delete/list/list_groups`.
 *
 * Free-function form of the previous `ISdk.createStream` instance method.
 */
export function createStream<TData>(iii: ISdk, streamName: string, stream: IStream<TData>): void {
  ;(iii as IIIWithHelperShims).__helpers_create_stream(streamName, stream)
}

/**
 * Register a custom trigger type with the engine.
 *
 * Free-function form of the previous `ISdk.registerTriggerType` method.
 */
export function registerTriggerType<TConfig>(
  iii: ISdk,
  triggerType: RegisterTriggerTypeInput,
  handler: TriggerHandler<TConfig>,
): TriggerTypeRef<TConfig> {
  return (iii as IIIWithHelperShims).__helpers_register_trigger_type(triggerType, handler)
}

/**
 * Unregister a previously registered trigger type by id.
 */
export function unregisterTriggerType(iii: ISdk, id: string): void {
  ;(iii as IIIWithHelperShims).__helpers_unregister_trigger_type(id)
}
