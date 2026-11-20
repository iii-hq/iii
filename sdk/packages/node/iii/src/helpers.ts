/**
 * Helper free functions that operate on an {@link IIIClient} instance: each one
 * takes the client as its first argument, so `IIIClient` itself keeps to the
 * connection lifecycle, functions, calls and observers.
 */
import type { IStream } from './stream'
import type { IIIClient } from './types'

type IIIWithHelperShims = IIIClient & {
  __helpers_create_stream<T>(name: string, stream: IStream<T>): void
}

/**
 * Register a custom stream implementation by wiring its 5 callable methods
 * to `stream::get/set/delete/list/list_groups`.
 *
 * @deprecated createStream is deprecated (iii-stream) and will be removed in an upcoming release. Behavior is unchanged for now. Migration guide: https://iii.dev/docs/upgrading/migrate-from-streams
 */
export function createStream<TData>(iii: IIIClient, streamName: string, stream: IStream<TData>): void {
  ;(iii as IIIWithHelperShims).__helpers_create_stream(streamName, stream)
}
