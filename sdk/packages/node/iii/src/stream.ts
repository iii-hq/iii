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
  /**
   * Per-op errors. Emitted by `merge` and `append` for validation
   * rejections (path depth/size, value depth, or a
   * `__proto__`/`constructor`/`prototype` segment or top-level key)
   * and by `append` for the case-2 `append.type_mismatch` and
   * `append.target_not_object` surfaces. Successfully applied ops are
   * still reflected in `new_value`. The field is omitted from the
   * JSON wire when empty.
   */
  errors?: UpdateOpError[]
}

/** Set a field at the given path to a value. */
export type UpdateSet = {
  type: 'set'
  /** First-level field path. Use an empty string to target the root value. */
  path: string
  /** Value to set. */
  // biome-ignore lint/suspicious/noExplicitAny: any is fine here
  value: any
}

/** Increment a numeric field by a given amount. */
export type UpdateIncrement = {
  type: 'increment'
  /** First-level field path. */
  path: string
  /** Amount to increment by. */
  by: number
}

/** Decrement a numeric field by a given amount. */
export type UpdateDecrement = {
  type: 'decrement'
  /** First-level field path. */
  path: string
  /** Amount to decrement by. */
  by: number
}

/**
 * Append an element to an array, concatenate a string, or push a new
 * value at a nested path. The target is the root (when `path` is
 * omitted, empty, or `[]`), a single first-level key (when `path` is
 * a non-empty string), or an arbitrary nested location (when `path`
 * is an array of literal segments).
 *
 * Engine semantics:
 *   - Missing or non-object intermediates along a nested path are
 *     auto-replaced with `{}` so a stray `null` or scalar never blocks
 *     future appends.
 *   - At the leaf:
 *       - missing/null + nested path → `[value]` (always an array)
 *       - missing/null + single-string path → string-as-string for the
 *         string-concat tier, otherwise `[value]`
 *       - existing array → push
 *       - existing string + string value → concatenate
 *       - existing object/scalar at the leaf → `append.type_mismatch`
 *   - Each path segment is a literal key. `["a.b"]` targets a single
 *     key named `"a.b"`, not `a → b`.
 *
 * Validation: invalid paths (depth > 32 segments, segment > 256
 * bytes, or any `__proto__`/`constructor`/`prototype` segment) are
 * rejected with a structured error in the `errors` field of the
 * `state::update` / `stream::update` response. The append does not
 * apply when an error is returned for that op.
 */
export type UpdateAppend = {
  type: 'append'
  /**
   * Optional path to the append target. Accepts a single first-level
   * key (legacy `string`) or an array of literal segments for nested
   * append. See {@link MergePath} (the same shape is reused).
   */
  path?: MergePath
  /** Value to append. String targets only accept string values. */
  // biome-ignore lint/suspicious/noExplicitAny: any is fine here
  value: any
}

/** Remove a field at the given path. */
export type UpdateRemove = {
  type: 'remove'
  /** First-level field path. */
  path: string
}

/**
 * Path target for a {@link UpdateMerge} op. Accepts:
 *   - a single string (legacy / first-level field)
 *   - an array of literal segments (nested path; each element is one
 *     literal key, dots are NOT interpreted as separators)
 *
 * Omit `path`, pass `""`, or pass `[]` to target the root value.
 *
 * @example single first-level field
 *   { type: 'merge', path: 'session-abc', value: { author: 'alice' } }
 * @example nested path (closes issue #1546's structured-state use case)
 *   { type: 'merge', path: ['sessions', 'abc'], value: { ts: chunk } }
 */
export type MergePath = string | string[]

/**
 * Shallow-merge an object into the target. The target is the root
 * (when `path` is omitted/empty) or an arbitrary nested location
 * specified by an array of literal segments.
 *
 * Engine semantics:
 *   - Missing or non-object intermediates along the path are
 *     auto-replaced with `{}` so a stray `null` or scalar never
 *     blocks future merges.
 *   - The merge is shallow at the target — top-level keys of `value`
 *     replace same-named keys; siblings are preserved.
 *   - Each path segment is a literal key. `["a.b"]` writes a single
 *     key named `"a.b"`, not `a → b`.
 *
 * Validation: invalid paths/values (depth > 32 segments, segment >
 * 256 bytes, value depth > 16, > 1024 top-level keys, or any
 * `__proto__`/`constructor`/`prototype` segment or top-level key)
 * are rejected with a structured error in the `errors` field of the
 * `state::update` / `stream::update` response. The merge does not
 * apply when an error is returned for that op.
 */
export type UpdateMerge = {
  type: 'merge'
  /** Optional path to the merge target. See {@link MergePath}. */
  path?: MergePath
  /** Object to merge. Must be a JSON object. */
  // biome-ignore lint/suspicious/noExplicitAny: any is fine here
  value: any
}

/** Per-op error returned by `state::update` / `stream::update`. */
export type UpdateOpError = {
  /** Index of the offending op within the original `ops` array. */
  op_index: number
  /** Stable error code, e.g. `"merge.path.too_deep"`. */
  code: string
  /** Human-readable description with concrete numbers when applicable. */
  message: string
  /** Optional documentation URL. */
  doc_url?: string
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
 *      {@link UpdateAppend}, {@link UpdateRemove}, {@link UpdateMerge}
 */
export type UpdateOp =
  | UpdateSet
  | UpdateIncrement
  | UpdateDecrement
  | UpdateAppend
  | UpdateRemove
  | UpdateMerge

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

/** Trigger config for `stream` triggers. Filters which item changes fire the handler. */
export interface StreamTriggerConfig {
  /** Stream name to watch. Only changes on this stream fire the handler. */
  stream_name: string
  /** If set, only changes within this group fire the handler. */
  group_id?: string
  /** If set, only changes to this specific item fire the handler. */
  item_id?: string
  /** Function ID for conditional execution. If it returns `false`, the handler is skipped. */
  condition_function_id?: string
}

/** Trigger config for `stream:join` and `stream:leave` triggers. */
export interface StreamJoinLeaveTriggerConfig {
  /** Function ID for conditional execution. If it returns `false`, the handler is skipped. */
  condition_function_id?: string
}

/** Handler input for `stream` triggers, fired when an item changes via `stream::set`, `stream::update`, or `stream::delete`. */
export interface StreamChangeEvent {
  /** The event type. */
  type: 'stream'
  /** Unix timestamp of the event. */
  timestamp: number
  /** The stream where the change occurred. */
  streamName: string
  /** The group where the change occurred. */
  groupId: string
  /** The item ID that changed. */
  id?: string
  /** The event detail object containing `type` and `data` fields. */
  event: {
    type: 'create' | 'update' | 'delete'
    // biome-ignore lint/suspicious/noExplicitAny: any is fine here
    data: any
  }
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
