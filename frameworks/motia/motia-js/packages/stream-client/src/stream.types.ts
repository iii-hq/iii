export type BaseMessage = {
  streamName: string
  groupId: string
  id?: string
  timestamp: number
}

export type JoinMessage = Omit<BaseMessage, 'timestamp'> & { subscriptionId: string }
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type CustomEvent = { type: string; data: any }
export type StreamEvent<TData extends { id: string }> =
  | { type: 'create'; data: TData }
  | { type: 'update'; data: TData }
  | { type: 'delete'; data: TData }
  | { type: 'event'; event: CustomEvent }
export type ItemStreamEvent<TData extends { id: string }> = StreamEvent<TData> | { type: 'sync'; data: TData }
export type GroupStreamEvent<TData extends { id: string }> = StreamEvent<TData> | { type: 'sync'; data: TData[] }
export type ItemEventMessage<TData extends { id: string }> = BaseMessage & { event: ItemStreamEvent<TData> }
export type GroupEventMessage<TData extends { id: string }> = BaseMessage & { event: GroupStreamEvent<TData> }

export type Message = { type: 'join' | 'leave'; data: JoinMessage }

export type Listener<TData> = (state: TData | null) => void
export type CustomEventListener<TData> = (event: TData) => void
