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
