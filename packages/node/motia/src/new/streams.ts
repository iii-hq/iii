import { StreamConfig } from '../types-stream'
import { bridge } from './bridge'

export class Stream<TData> {
  constructor(readonly config: StreamConfig) {}

  async get(groupId: string, itemId: string): Promise<TData | null> {
    return bridge.invokeFunction('streams.get', {
      stream_name: this.config.name,
      group_id: groupId,
      item_id: itemId,
    })
  }

  async set(groupId: string, itemId: string, data: TData): Promise<TData> {
    return bridge.invokeFunction('streams.set', {
      stream_name: this.config.name,
      group_id: groupId,
      item_id: itemId,
      data,
    })
  }

  async delete(groupId: string, itemId: string): Promise<void> {
    return bridge.invokeFunction('streams.delete', {
      stream_name: this.config.name,
      group_id: groupId,
      item_id: itemId,
    })
  }

  async getGroup(groupId: string): Promise<TData[]> {
    return bridge.invokeFunction('streams.getGroup', {
      stream_name: this.config.name,
      group_id: groupId,
    })
  }
}
