import { create } from 'zustand'
import { deriveTraceGroup } from '../hooks/use-derive-trace-group'
import type { Trace, TraceGroup, TraceGroupMeta } from '../types/observability'

export type ObservabilityState = {
  traceGroupMetas: TraceGroupMeta[]
  traces: Trace[]
  tracesByGroupId: Record<string, Trace[]>
  selectedTraceGroupId: string
  selectedTraceId?: string
  search: string
  setTraceGroupMetas: (metas: TraceGroupMeta[]) => void
  setTraces: (traces: Trace[]) => void
  setTracesForGroup: (groupId: string, traces: Trace[]) => void
  selectTraceGroupId: (groupId?: string) => void
  selectTraceId: (traceId?: string) => void
  setSearch: (search: string) => void
  clearTraces: () => void
  getTraceGroups: () => TraceGroup[]
}

export const useObservabilityStore = create<ObservabilityState>()((set, get) => ({
  traceGroupMetas: [],
  traces: [],
  tracesByGroupId: {},
  selectedTraceGroupId: '',
  selectedTraceId: undefined,
  search: '',
  setTraceGroupMetas: (metas: TraceGroupMeta[]) => {
    const safeMetas = Array.isArray(metas) ? metas : []
    set({ traceGroupMetas: safeMetas })
  },
  setTraces: (traces: Trace[]) => {
    const safeTraces = Array.isArray(traces) ? traces : []
    set({ traces: safeTraces })
  },
  setTracesForGroup: (groupId: string, traces: Trace[]) => {
    const state = get()
    const safeTraces = Array.isArray(traces) ? [...traces] : []
    const newTracesByGroupId = { ...state.tracesByGroupId, [groupId]: safeTraces }
    const newTraces = state.selectedTraceGroupId === groupId ? safeTraces : state.traces
    set({
      tracesByGroupId: newTracesByGroupId,
      traces: newTraces,
    })
  },
  selectTraceGroupId: (groupId) => {
    const state = get()
    const traces = groupId ? state.tracesByGroupId[groupId] || [] : []
    set({ selectedTraceGroupId: groupId || '', traces })
  },
  selectTraceId: (traceId) => set({ selectedTraceId: traceId }),
  setSearch: (search) => set({ search }),
  clearTraces: () => {
    fetch('/__motia/trace/clear', { method: 'POST' })
  },
  getTraceGroups: () => {
    const state = get()
    return state.traceGroupMetas.map((meta) => {
      const traces = state.tracesByGroupId[meta.id] || []
      if (traces.length === 0) {
        return {
          ...meta,
          status: 'running' as const,
          lastActivity: meta.startTime,
          metadata: {
            completedSteps: 0,
            activeSteps: 0,
            totalSteps: 0,
          },
        }
      }
      return deriveTraceGroup(meta, traces)
    })
  },
}))
