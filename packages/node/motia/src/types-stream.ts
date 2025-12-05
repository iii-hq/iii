import type { StepSchemaInput } from './types'

export type StreamSubscription = { groupId: string; id?: string }

export interface StreamConfig {
  name: string
  schema: StepSchemaInput
  baseConfig: { storageType: 'default' } | { storageType: 'custom'; factory: () => MotiaStream<any> }
  canAccess?: (subscription: StreamSubscription, authContext: any) => boolean | Promise<boolean>
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
