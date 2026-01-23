export interface StreamAuthInput {
  headers: Record<string, string>
  path: string
  query_params: Record<string, string[]>
  addr: string
}

export interface StreamAuthResult {
  context?: any
}

export type StreamContext = StreamAuthResult['context']

export interface StreamJoinLeaveEvent {
  subscription_id: string
  stream_name: string
  group_id: string
  id?: string
  context?: StreamContext
}

export interface StreamJoinResult {
  unauthorized: boolean
}

export type StreamGetInput = {
  stream_name: string
  group_id: string
  item_id: string
}

export type StreamSetInput = {
  stream_name: string
  group_id: string
  item_id: string
  data: any
}

export type StreamDeleteInput = {
  stream_name: string
  group_id: string
  item_id: string
}

export type StreamGetGroupInput = {
  stream_name: string
  group_id: string
}

export type StreamListGroupsInput = {
  stream_name: string
}

export type StreamSetResult<TData> = {
  old_value?: TData
  new_value?: TData
}


export type UpdateSet = {
  type: 'set'
  path: string
  value: any
}

export type UpdateIncrement = {
  type: 'increment'
  path: string
  by: number
}

export type UpdateDecrement = {
  type: 'decrement'
  path: string
  by: number
}

export type UpdateRemove = {
  type: 'remove'
  path: string
}

export type UpdateMerge = {
  type: 'merge'
  path: string
  value: any
}

export type UpdateOp = UpdateSet | UpdateIncrement | UpdateDecrement | UpdateRemove | UpdateMerge

export type StreamUpdateInput = {
  stream_name: string
  group_id: string
  item_id: string
  ops: UpdateOp[]
}

export interface IStream<TData> {
  get(input: StreamGetInput): Promise<TData | null>
  set(input: StreamSetInput): Promise<StreamSetResult<TData> | null>
  delete(input: StreamDeleteInput): Promise<void>
  getGroup(input: StreamGetGroupInput): Promise<TData[]>
  listGroups(input: StreamListGroupsInput): Promise<string[]>
  update(input: StreamUpdateInput): Promise<StreamSetResult<TData> | null>
}
