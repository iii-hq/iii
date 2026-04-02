/** Input for stream authentication. */
export interface StreamAuthInput {
  /** Request headers. */
  headers: Record<string, string>
  /** Request path. */
  path: string
  /** Query parameters. */
  query_params: Record<string, string[]>
  /** Client address. */
  addr: string
}

/** Result of stream authentication. */
export interface StreamAuthResult {
  /** Arbitrary context passed to stream handlers after authentication. */
  // biome-ignore lint/suspicious/noExplicitAny: any is fine here
  context?: any
}

/** Context type extracted from {@link StreamAuthResult}. */
export type StreamContext = StreamAuthResult['context']

/** Event payload for stream join/leave events. */
export interface StreamJoinLeaveEvent {
  /** Unique subscription identifier. */
  subscription_id: string
  /** Name of the stream. */
  stream_name: string
  /** Group identifier. */
  group_id: string
  /** Item identifier (if applicable). */
  id?: string
  /** Auth context from {@link StreamAuthResult}. */
  context?: StreamContext
}

/** Result of a stream join request. */
export interface StreamJoinResult {
  /** Whether the join was unauthorized. */
  unauthorized: boolean
}

/** Input for retrieving a single stream item. */
export type StreamGetInput = {
  /** Name of the stream. */
  stream_name: string
  /** Group identifier. */
  group_id: string
  /** Item identifier. */
  item_id: string
}

/** Input for setting a stream item. */
export type StreamSetInput = {
  /** Name of the stream. */
  stream_name: string
  /** Group identifier. */
  group_id: string
  /** Item identifier. */
  item_id: string
  /** Data to store. */
  // biome-ignore lint/suspicious/noExplicitAny: any is fine here
  data: any
}

/** Input for deleting a stream item. */
export type StreamDeleteInput = {
  /** Name of the stream. */
  stream_name: string
  /** Group identifier. */
  group_id: string
  /** Item identifier. */
  item_id: string
}

/** Input for listing all items in a stream group. */
export type StreamListInput = {
  /** Name of the stream. */
  stream_name: string
  /** Group identifier. */
  group_id: string
}

/** Input for listing all groups in a stream. */
export type StreamListGroupsInput = {
  /** Name of the stream. */
  stream_name: string
}

/** Result of a stream set operation. */
export type StreamSetResult<TData> = {
  /** Previous value (if it existed). */
  old_value?: TData
  /** New value that was stored. */
  new_value: TData
}

/** Result of a stream update operation. */
export type StreamUpdateResult<TData> = {
  /** Previous value (if it existed). */
  old_value?: TData
  /** New value after the update. */
  new_value: TData
}

/** Set a field at the given path to a value. */
export type UpdateSet = {
  type: 'set'
  /** Dot-separated field path (e.g. `user.name`). */
  path: string
  /** Value to set. */
  // biome-ignore lint/suspicious/noExplicitAny: any is fine here
  value: any
}

/** Increment a numeric field by a given amount. */
export type UpdateIncrement = {
  type: 'increment'
  /** Dot-separated field path. */
  path: string
  /** Amount to increment by. */
  by: number
}

/** Decrement a numeric field by a given amount. */
export type UpdateDecrement = {
  type: 'decrement'
  /** Dot-separated field path. */
  path: string
  /** Amount to decrement by. */
  by: number
}

/** Remove a field at the given path. */
export type UpdateRemove = {
  type: 'remove'
  /** Dot-separated field path. */
  path: string
}

/** Deep-merge an object into the field at the given path. */
export type UpdateMerge = {
  type: 'merge'
  /** Dot-separated field path. */
  path: string
  /** Object to merge. */
  // biome-ignore lint/suspicious/noExplicitAny: any is fine here
  value: any
}

/** Result of a stream delete operation. */
export type DeleteResult = {
  /** Previous value (if it existed). */
  // biome-ignore lint/suspicious/noExplicitAny: any is fine here
  old_value?: any
}

/**
 * Union of all atomic update operations supported by streams.
 *
 * @see {@link UpdateSet}, {@link UpdateIncrement}, {@link UpdateDecrement},
 *      {@link UpdateRemove}, {@link UpdateMerge}
 */
export type UpdateOp = UpdateSet | UpdateIncrement | UpdateDecrement | UpdateRemove | UpdateMerge

/** Input for atomically updating a stream item. */
export type StreamUpdateInput = {
  /** Name of the stream. */
  stream_name: string
  /** Group identifier. */
  group_id: string
  /** Item identifier. */
  item_id: string
  /** Ordered list of update operations to apply atomically. */
  ops: UpdateOp[]
}

/**
 * Interface for custom stream implementations. Passed to `ISdk.createStream`
 * to override the engine's built-in stream storage.
 *
 * @typeParam TData - Type of the data stored in the stream.
 */
export interface IStream<TData> {
  /** Retrieve a single item by group and item ID. */
  get(input: StreamGetInput): Promise<TData | null>
  /** Set (create or overwrite) a stream item. */
  set(input: StreamSetInput): Promise<StreamSetResult<TData> | null>
  /** Delete a stream item. */
  delete(input: StreamDeleteInput): Promise<DeleteResult>
  /** List all items in a group. */
  list(input: StreamListInput): Promise<TData[]>
  /** List all group IDs in a stream. */
  listGroups(input: StreamListGroupsInput): Promise<string[]>
  /** Apply atomic update operations to a stream item. */
  update(input: StreamUpdateInput): Promise<StreamUpdateResult<TData> | null>
}
