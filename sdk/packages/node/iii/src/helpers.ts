/**
 * Helper free functions that operate on an {@link IIIClient} instance.
 *
 * These were previously instance methods on the SDK. They take the iii
 * instance as the first argument so the public API surface of `IIIClient` stays
 * focused on the core lifecycle and registration methods.
 */
import type { Channel, IIIClient } from './types'
import type { IStream } from './stream'

export { ChannelDirection, ChannelItem } from './channels'
export { extractChannelRefs, isChannelRef } from './utils'

type IIIWithHelperShims = IIIClient & {
  __helpers_create_channel(bufferSize?: number): Promise<Channel>
  __helpers_create_stream<T>(name: string, stream: IStream<T>): void
}

/**
 * Create a streaming channel pair for worker-to-worker data transfer.
 *
 * Free-function form of the previous `IIIClient.createChannel` instance method.
 */
export function createChannel(iii: IIIClient, bufferSize?: number): Promise<Channel> {
  return (iii as IIIWithHelperShims).__helpers_create_channel(bufferSize)
}

/**
 * Register a custom stream implementation by wiring its 5 callable methods
 * to `stream::get/set/delete/list/list_groups`.
 *
 * Free-function form of the previous `IIIClient.createStream` instance method.
 */
export function createStream<TData>(iii: IIIClient, streamName: string, stream: IStream<TData>): void {
  ;(iii as IIIWithHelperShims).__helpers_create_stream(streamName, stream)
}
