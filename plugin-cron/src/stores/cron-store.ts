import { create } from 'zustand'
import type { CronExecution, CronFilter, CronJob, CronStats } from '../types/cron'

const MAX_EXECUTIONS = 100

interface CronStore {
  jobs: CronJob[]
  executions: CronExecution[]
  stats: CronStats
  filter: CronFilter
  selectedJobId: string | null
  isLoading: boolean
  lastRefresh: number | null
  error: string | null

  setJobs: (jobs: CronJob[]) => void
  updateJob: (id: string, updates: Partial<CronJob>) => void
  addExecution: (execution: CronExecution) => void
  updateExecution: (id: string, updates: Partial<CronExecution>) => void
  setStats: (stats: CronStats) => void
  setFilter: (filter: Partial<CronFilter>) => void
  selectJob: (id: string | null) => void
  setLoading: (loading: boolean) => void
  setError: (error: string | null) => void
  clearExecutions: (jobId?: string) => void
  getFilteredJobs: () => CronJob[]
  getJobExecutions: (jobId: string) => CronExecution[]
}

export const useCronStore = create<CronStore>((set, get) => ({
  jobs: [],
  executions: [],
  stats: {
    totalJobs: 0,
    runningJobs: 0,
    failedInLast24h: 0,
    executionsToday: 0,
    avgExecutionTime: 0,
  },
  filter: {
    status: 'all',
    search: '',
    flow: undefined,
  },
  selectedJobId: null,
  isLoading: false,
  lastRefresh: null,
  error: null,

  setJobs: (jobs) =>
    set((state) => ({
      jobs,
      lastRefresh: Date.now(),
      stats: {
        ...state.stats,
        totalJobs: jobs.length,
        runningJobs: jobs.filter((j) => j.status === 'running').length,
      },
    })),

  updateJob: (id, updates) =>
    set((state) => ({
      jobs: state.jobs.map((j) => (j.id === id ? { ...j, ...updates } : j)),
    })),

  addExecution: (execution) =>
    set((state) => {
      const executions = [execution, ...state.executions].slice(0, MAX_EXECUTIONS)
      return {
        executions,
        stats: {
          ...state.stats,
          runningJobs: execution.status === 'running' ? state.stats.runningJobs + 1 : state.stats.runningJobs,
        },
      }
    }),

  updateExecution: (id, updates) =>
    set((state) => {
      const executions = state.executions.map((e) => (e.id === id ? { ...e, ...updates } : e))
      const runningCount = executions.filter((e) => e.status === 'running').length
      return {
        executions,
        stats: { ...state.stats, runningJobs: runningCount },
      }
    }),

  setStats: (stats) => set({ stats }),
  setFilter: (filter) => set((state) => ({ filter: { ...state.filter, ...filter } })),
  selectJob: (id) => set({ selectedJobId: id }),
  setLoading: (isLoading) => set({ isLoading }),
  setError: (error) => set({ error }),

  clearExecutions: (jobId) =>
    set((state) => ({
      executions: jobId ? state.executions.filter((e) => e.jobId !== jobId) : [],
    })),

  getFilteredJobs: () => {
    const state = get()
    let filtered = state.jobs

    if (state.filter.status && state.filter.status !== 'all') {
      filtered = filtered.filter((j) => j.status === state.filter.status)
    }

    if (state.filter.search) {
      const search = state.filter.search.toLowerCase()
      filtered = filtered.filter(
        (j) =>
          j.name.toLowerCase().includes(search) ||
          j.description?.toLowerCase().includes(search) ||
          j.cronExpression.includes(search),
      )
    }

    if (state.filter.flow) {
      filtered = filtered.filter((j) => j.flows.includes(state.filter.flow!))
    }

    return filtered
  },

  getJobExecutions: (jobId) => get().executions.filter((e) => e.jobId === jobId),
}))
