import { bridge } from './bridge'

export const state = {
  get: async (group_id: string, item_id: string): Promise<any | null> => {
    return bridge.invokeFunction('state.get', { group_id, item_id })
  },
  set: async (group_id: string, item_id: string, data: any): Promise<any> => {
    return bridge.invokeFunction('state.set', { group_id, item_id, data })
  },
  delete: async (group_id: string, item_id: string): Promise<void> => {
    return bridge.invokeFunction('state.delete', { group_id, item_id })
  },
  getGroup: async (group_id: string): Promise<any[]> => {
    return bridge.invokeFunction('state.getGroup', { group_id })
  },
}
