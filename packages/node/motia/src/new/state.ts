import { InternalStateManager } from '../types'
import { bridge } from './bridge'

const STREAM_NAME = '$$internal-state'

export class StateManager implements InternalStateManager {
  async get<T>(groupId: string, itemId: string): Promise<T | null> {
    return bridge.invokeFunction('streams.get', {
      stream_name: STREAM_NAME,
      group_id: groupId,
      item_id: itemId,
    })
  }

  async set<T>(groupId: string, itemId: string, data: T): Promise<T> {
    return bridge.invokeFunction('streams.set', {
      stream_name: STREAM_NAME,
      group_id: groupId,
      item_id: itemId,
      data,
    })
  }

  async delete<T>(groupId: string, itemId: string): Promise<T | null> {
    return bridge.invokeFunction('streams.delete', {
      stream_name: STREAM_NAME,
      group_id: groupId,
      item_id: itemId,
    })
  }

  async getGroup<T>(groupId: string): Promise<T[]> {
    return bridge.invokeFunction('streams.getGroup', {
      stream_name: STREAM_NAME,
      group_id: groupId,
    })
  }

  async clear(groupId: string): Promise<void> {
    const items = await this.getGroup<{ id: string }>(groupId)

    for (const item of items) {
      await this.delete(groupId, item.id)
    }
  }
}
