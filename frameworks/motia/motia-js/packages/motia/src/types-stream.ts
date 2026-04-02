import type { StreamAuthResult, StreamContext, StreamJoinResult, StreamSetResult, UpdateOp } from 'iii-sdk/stream'
import type { FlowContext, StepSchemaInput } from './types'

export type {
  StreamAuthResult,
  StreamContext,
  StreamJoinResult,
  StreamSetInput,
  StreamSetResult,
  UpdateOp,
} from 'iii-sdk/stream'
export type StreamSubscription = { groupId: string; id?: string }

export interface StreamAuthInput {
  headers: Record<string, string>
  path: string
  queryParams: Record<string, string[]>
  addr: string
}

export type AuthenticateStream = (input: StreamAuthInput, context: FlowContext) => Promise<StreamAuthResult>

type PromiseOrValue<T> = T | Promise<T>

export interface StreamConfig {
  name: string
  schema: StepSchemaInput
  baseConfig: { storageType: 'default' }
  onJoin?: (
    subscription: StreamSubscription,
    context: FlowContext,
    authContext?: StreamContext,
  ) => PromiseOrValue<StreamJoinResult>
  onLeave?: (
    subscription: StreamSubscription,
    context: FlowContext,
    authContext?: StreamContext,
  ) => PromiseOrValue<void>
}

export type StateStreamEventChannel = { groupId: string; id?: string }
export type StateStreamEvent<TData> = { type: string; data: TData }

export type BaseStreamItem<TData = unknown> = TData & { id: string }

export type Stream<TConfig extends StreamConfig = StreamConfig> = {
  filePath: string
  config: TConfig
  hidden?: boolean
}

export interface MotiaStream<TData> {
  get(groupId: string, id: string): Promise<BaseStreamItem<TData> | null>
  set(groupId: string, id: string, data: TData): Promise<StreamSetResult<BaseStreamItem<TData>>>
  delete(groupId: string, id: string): Promise<BaseStreamItem<TData> | null>
  getGroup(groupId: string): Promise<BaseStreamItem<TData>[]>
  update(groupId: string, id: string, data: UpdateOp[]): Promise<StreamSetResult<BaseStreamItem<TData>>>

  send<T>(channel: StateStreamEventChannel, event: StateStreamEvent<T>): Promise<void>
}
