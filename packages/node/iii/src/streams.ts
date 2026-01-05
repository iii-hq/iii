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

export type StreamSetResult<TData> = {
  existed: boolean
  data?: TData
}

export interface IStream<TData> {
  get(input: StreamGetInput): Promise<TData | null>
  set(input: StreamSetInput): Promise<StreamSetResult<TData> | null>
  delete(input: StreamDeleteInput): Promise<void>
  getGroup(input: StreamGetGroupInput): Promise<TData[]>
}
