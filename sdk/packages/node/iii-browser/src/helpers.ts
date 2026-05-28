/**
 * Helper free functions that operate on an {@link ISdk} instance.
 *
 * These were previously instance methods on the SDK. They take the iii
 * instance as the first argument so the public API surface of `ISdk` stays
 * focused on the core lifecycle and registration methods.
 */
import type { Channel, ISdk } from './types'
import type { IStream } from './stream'

export { ChannelDirection, ChannelItem } from './channels'
export { extractChannelRefs, isChannelRef } from './utils'

type IIIWithHelperShims = ISdk & {
  __helpers_create_channel(bufferSize?: number): Promise<Channel>
  __helpers_create_stream<T>(name: string, stream: IStream<T>): void
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
