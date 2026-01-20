import { create } from 'zustand'

export type StatesState = {
  selectedStateId?: string
  selectStateId: (stateId?: string) => void
}

export const useStatesStore = create<StatesState>()((set) => ({
  selectedStateId: undefined,
  selectStateId: (stateId) => set({ selectedStateId: stateId }),
}))
