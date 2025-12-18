import { StreamAuthResult, StreamContext, StreamJoinResult } from '@iii-dev/sdk'
import type { FlowContext, StepSchemaInput } from './types'

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
  baseConfig: { storageType: 'default' } | { storageType: 'custom'; factory: () => MotiaStream<any> }
  /**
   * @deprecated Use onJoin instead
   */
  canAccess?: (subscription: StreamSubscription, authContext: StreamContext) => boolean | Promise<boolean>

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
  set(groupId: string, id: string, data: TData): Promise<BaseStreamItem<TData>>
  delete(groupId: string, id: string): Promise<BaseStreamItem<TData> | null>
  getGroup(groupId: string): Promise<BaseStreamItem<TData>[]>

  send<T>(channel: StateStreamEventChannel, event: StateStreamEvent<T>): Promise<void>
}
