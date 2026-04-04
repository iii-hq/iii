import type { UpdateOp } from './stream'

/** Input for retrieving a state value. */
export type StateGetInput = {
  /** State scope (namespace). */
  scope: string
  /** Key within the scope. */
  key: string
}

/** Input for setting a state value. */
export type StateSetInput = {
  /** State scope (namespace). */
  scope: string
  /** Key within the scope. */
  key: string
  /** Value to store. */
  // biome-ignore lint/suspicious/noExplicitAny: any is fine here
  value: any
}

/** Input for deleting a state value. */
export type StateDeleteInput = {
  /** State scope (namespace). */
  scope: string
  /** Key within the scope. */
  key: string
}

/** Result of a state delete operation. */
export type StateDeleteResult = {
  /** Previous value (if it existed). */
  // biome-ignore lint/suspicious/noExplicitAny: any is fine here
  old_value?: any
}

/** Input for listing all values in a state scope. */
export type StateListInput = {
  /** State scope (namespace). */
  scope: string
}

/** Result of a state set operation. */
export type StateSetResult<TData> = {
  /** Previous value (if it existed). */
  old_value?: TData
  /** New value that was stored. */
  new_value: TData
}

/** Result of a state update operation. */
export type StateUpdateResult<TData> = {
  /** Previous value (if it existed). */
  old_value?: TData
  /** New value after the update. */
  new_value: TData
}

/** Result of a state delete operation. */
export type DeleteResult = {
  /** Previous value (if it existed). */
  // biome-ignore lint/suspicious/noExplicitAny: any is fine here
  old_value?: any
}

/** Input for atomically updating a state value. */
export type StateUpdateInput = {
  /** State scope (namespace). */
  scope: string
  /** Key within the scope. */
  key: string
  /** Ordered list of update operations to apply atomically. */
  ops: UpdateOp[]
}

/** Types of state change events. */
export enum StateEventType {
  Created = 'state:created',
  Updated = 'state:updated',
  Deleted = 'state:deleted',
}

/** Payload for state change events. */
// biome-ignore lint/suspicious/noExplicitAny: any is fine here
export interface StateEventData<TData = any> {
  type: 'state'
  /** Type of state change. */
  event_type: StateEventType
  /** State scope (namespace). */
  scope: string
  /** Key within the scope. */
  key: string
  /** Previous value (for update/delete events). */
  old_value?: TData
  /** New value (for create/update events). */
  new_value?: TData
}

/**
 * Interface for state management operations. Available via the `iii-sdk/state`
 * subpath export.
 */
export interface IState {
  /** Retrieve a value by scope and key. */
  get<TData>(input: StateGetInput): Promise<TData | null>
  /** Set (create or overwrite) a state value. */
  set<TData>(input: StateSetInput): Promise<StateSetResult<TData> | null>
  /** Delete a state value. */
  delete(input: StateDeleteInput): Promise<DeleteResult>
  /** List all values in a scope. */
  list<TData>(input: StateListInput): Promise<TData[]>
  /** Apply atomic update operations to a state value. */
  update<TData>(input: StateUpdateInput): Promise<StateUpdateResult<TData> | null>
}
