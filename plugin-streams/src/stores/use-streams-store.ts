import { create } from 'zustand'
import type { StreamItem, StreamViewMode } from '../types/stream'

export interface StreamsState {
  selectedStreamId: string | null
  selectedGroupId: string | null
  selectedItemId: string | null

  viewMode: StreamViewMode
  search: string
  showHidden: boolean
  expandedGroups: Set<string>

  selectedGroupItems: StreamItem[]

  selectStream: (streamId: string | null) => void
  selectGroup: (groupId: string | null) => void
  selectItem: (itemId: string | null) => void
  setViewMode: (mode: StreamViewMode) => void
  setSearch: (search: string) => void
  toggleShowHidden: () => void
  toggleGroupExpanded: (groupId: string) => void
  setSelectedGroupItems: (items: StreamItem[]) => void
  reset: () => void
}

const initialState = {
  selectedStreamId: null,
  selectedGroupId: null,
  selectedItemId: null,
  viewMode: 'list' as StreamViewMode,
  search: '',
  showHidden: false,
  expandedGroups: new Set<string>(),
  selectedGroupItems: [],
}

export const useStreamsStore = create<StreamsState>((set) => ({
  ...initialState,

  selectStream: (streamId) =>
    set({
      selectedStreamId: streamId,
      selectedGroupId: null,
      selectedItemId: null,
      selectedGroupItems: [],
    }),

  selectGroup: (groupId) =>
    set({
      selectedGroupId: groupId,
      selectedItemId: null,
      selectedGroupItems: [],
    }),

  selectItem: (itemId) => set({ selectedItemId: itemId }),

  setViewMode: (mode) => set({ viewMode: mode }),

  setSearch: (search) => set({ search }),

  toggleShowHidden: () => set((state) => ({ showHidden: !state.showHidden })),

  toggleGroupExpanded: (groupId) =>
    set((state) => {
      const expandedGroups = new Set(state.expandedGroups)
      if (expandedGroups.has(groupId)) {
        expandedGroups.delete(groupId)
      } else {
        expandedGroups.add(groupId)
      }
      return { expandedGroups }
    }),

  setSelectedGroupItems: (items) => set({ selectedGroupItems: items }),

  reset: () => set(initialState),
}))
