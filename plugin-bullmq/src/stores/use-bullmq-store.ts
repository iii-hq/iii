import { create } from 'zustand'
import type { JobInfo, JobStatus, QueueInfo } from '../types/queue'

type BullMQState = {
  queues: QueueInfo[]
  selectedQueue: QueueInfo | null
  selectedJob: JobInfo | null
  selectedStatus: JobStatus
  error: string | null
  searchQuery: string
  jobDetailOpen: boolean

  setQueues: (queues: QueueInfo[]) => void
  setSelectedQueue: (queue: QueueInfo | null) => void
  updateSelectedQueueStats: (queue: QueueInfo) => void
  setSelectedJob: (job: JobInfo | null) => void
  setSelectedStatus: (status: JobStatus) => void
  setError: (error: string | null) => void
  setSearchQuery: (query: string) => void
  setJobDetailOpen: (open: boolean) => void
  reset: () => void
}

const initialState = {
  queues: [],
  selectedQueue: null,
  selectedJob: null,
  selectedStatus: 'waiting' as JobStatus,
  error: null,
  searchQuery: '',
  jobDetailOpen: false,
}

export const useBullMQStore = create<BullMQState>((set) => ({
  ...initialState,

  setQueues: (queues) => set({ queues }),
  setSelectedQueue: (queue) => set({ selectedQueue: queue, selectedJob: null }),
  updateSelectedQueueStats: (queue) => set({ selectedQueue: queue }),
  setSelectedJob: (job) => set({ selectedJob: job }),
  setSelectedStatus: (status) => set({ selectedStatus: status }),
  setError: (error) => set({ error }),
  setSearchQuery: (query) => set({ searchQuery: query }),
  setJobDetailOpen: (open) => set({ jobDetailOpen: open }),
  reset: () => set(initialState),
}))
