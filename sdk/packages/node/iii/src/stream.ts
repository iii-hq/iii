import type {
  StreamDeleteInput,
  StreamDeleteResult,
  StreamGetInput,
  StreamListGroupsInput,
  StreamListInput,
  StreamSetInput,
  StreamSetResult,
  StreamUpdateInput,
  StreamUpdateResult,
} from '@iii-dev/helpers/stream'

/**
 * Interface for custom stream implementations. Passed to `IIIClient.createStream`
 * to override the engine's built-in stream storage.
 *
 * @typeParam TData - Type of the data stored in the stream.
 *
 * @deprecated IStream is deprecated (iii-stream) and will be removed in an upcoming release. Behavior is unchanged for now. Migration guide: https://iii.dev/docs/upgrading/migrate-from-streams
 */
export interface IStream<TData> {
  /** Retrieve a single item by group and item ID. */
  get(input: StreamGetInput): Promise<TData | null>
  /** Set (create or overwrite) a stream item. */
  set(input: StreamSetInput): Promise<StreamSetResult<TData> | null>
  /** Delete a stream item. */
  delete(input: StreamDeleteInput): Promise<StreamDeleteResult>
  /** List all items in a group. */
  list(input: StreamListInput): Promise<TData[]>
  /** List all group IDs in a stream. */
  listGroups(input: StreamListGroupsInput): Promise<string[]>
  /** Apply atomic update operations to a stream item. */
  update(input: StreamUpdateInput): Promise<StreamUpdateResult<TData> | null>
}
