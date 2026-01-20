import { create } from 'zustand'
import type { Log } from '../types/log'

export type LogsState = {
  logs: Log[]
  selectedLogId?: string
  setLogs: (logs: Log[]) => void
  resetLogs: () => void
  selectLogId: (logId?: string) => void
}

export const useLogsStore = create<LogsState>()((set) => ({
  logs: [],
  selectedLogId: undefined,
  setLogs: (logs: Log[]) =>
    set(() => {
      const safeLogs = Array.isArray(logs) ? logs : []
      return {
        logs: [...safeLogs].reverse(),
      }
    }),
  resetLogs: () => {
    set({ logs: [] })
  },
  selectLogId: (logId) => set({ selectedLogId: logId }),
}))
