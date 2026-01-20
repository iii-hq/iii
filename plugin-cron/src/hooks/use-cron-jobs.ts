import { useStreamGroup } from '@motiadev/stream-client-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useCronStore } from '../stores/cron-store'
import type { CronExecution, CronJob } from '../types/cron'

interface TraceGroup {
  id: string
  correlationId?: string
  name: string
  status: 'running' | 'completed' | 'failed'
  startTime: number
  endTime?: number
  lastActivity: number
  metadata: {
    completedSteps: number
    activeSteps: number
    totalSteps: number
  }
}

function _parseCronExpression(expression: string): string {
  const parts = expression.trim().split(/\s+/)
  if (parts.length !== 5) return expression

  const [minute, hour, , , dayOfWeek] = parts

  if (minute === '*' && hour === '*') return 'Every minute'
  if (minute.startsWith('*/')) return `Every ${minute.slice(2)} minutes`
  if (minute === '0' && hour === '*') return 'Every hour'
  if (minute === '0' && hour === '0') return 'Daily at midnight'
  if (minute === '0' && hour === '9') return 'Daily at 9:00 AM'
  if (dayOfWeek === '1-5') return `Weekdays at ${hour}:${minute.padStart(2, '0')}`
  if (dayOfWeek === '1') return `Every Monday at ${hour}:${minute.padStart(2, '0')}`

  return expression
}

function calculateNextRun(expression: string): number {
  const now = Date.now()
  const parts = expression.trim().split(/\s+/)

  if (parts[0] === '*') {
    const seconds = new Date().getSeconds()
    return now + (60 - seconds) * 1000
  }

  if (parts[0].startsWith('*/')) {
    const interval = parseInt(parts[0].slice(2), 10)
    const minutes = new Date().getMinutes()
    const nextMinute = Math.ceil((minutes + 1) / interval) * interval
    const diff = nextMinute - minutes
    return now + diff * 60_000
  }

  return now + 60_000
}

export function useCronMonitor() {
  const [isConnected] = useState(true)
  const { setLoading, setError } = useCronStore()
  const discoveredJobsRef = useRef<Set<string>>(new Set())

  const streamGroupArgs = useMemo(() => ({ streamName: 'motia-trace-group', groupId: 'default' }), [])
  const { data: traceGroups } = useStreamGroup<TraceGroup & { id: string }>(streamGroupArgs)

  const cronTraceGroups = useMemo(() => {
    if (!traceGroups) return []
    return traceGroups.filter((group) => group.name.includes('Cron'))
  }, [traceGroups])

  useEffect(() => {
    if (!cronTraceGroups || cronTraceGroups.length === 0) return

    cronTraceGroups.forEach((group) => {
      const store = useCronStore.getState()

      if (!discoveredJobsRef.current.has(group.name)) {
        discoveredJobsRef.current.add(group.name)

        const existingJob = store.jobs.find((j) => j.name === group.name)
        if (!existingJob) {
          const newJob: CronJob = {
            id: group.name,
            name: group.name,
            description: `Cron step: ${group.name}`,
            cronExpression: '* * * * *',
            cronDescription: 'Every minute',
            status: group.status === 'running' ? 'running' : 'idle',
            flows: [],
            nextRunAt: calculateNextRun('* * * * *'),
            runCount: 0,
            errorCount: 0,
            enabled: true,
            createdAt: group.startTime,
          }
          store.setJobs([...store.jobs, newJob])
        }
      }

      const exec: CronExecution = {
        id: group.id,
        jobId: group.name,
        jobName: group.name,
        startedAt: group.startTime,
        completedAt: group.endTime,
        duration: group.endTime ? group.endTime - group.startTime : undefined,
        status: group.status,
        traceId: group.id,
      }

      const existingExec = store.executions.find((e) => e.id === exec.id)
      if (!existingExec) {
        store.addExecution(exec)
      } else if (existingExec.status !== exec.status) {
        store.updateExecution(exec.id, {
          status: exec.status,
          completedAt: exec.completedAt,
          duration: exec.duration,
        })
      }

      const job = store.jobs.find((j) => j.name === group.name)
      if (job) {
        if (group.status === 'running' && job.status !== 'running') {
          store.updateJob(job.id, { status: 'running', lastRunAt: group.startTime })
        } else if (group.status === 'completed' && job.status === 'running') {
          store.updateJob(job.id, {
            status: 'idle',
            lastDuration: group.endTime ? group.endTime - group.startTime : undefined,
            runCount: job.runCount + 1,
            nextRunAt: calculateNextRun(job.cronExpression),
          })
        } else if (group.status === 'failed') {
          store.updateJob(job.id, {
            status: 'failed',
            errorCount: job.errorCount + 1,
            runCount: job.runCount + 1,
          })
        }
      }
    })

    setLoading(false)
  }, [cronTraceGroups, setLoading])

  const refresh = useCallback(() => {
    setLoading(true)
    setTimeout(() => setLoading(false), 500)
  }, [setLoading])

  const triggerJob = useCallback(
    async (_jobId: string) => {
      setError('Manual trigger not available - cron jobs run on schedule')
    },
    [setError],
  )

  const toggleJob = useCallback((jobId: string, enabled: boolean) => {
    useCronStore.getState().updateJob(jobId, {
      enabled,
      status: enabled ? 'idle' : 'disabled',
    })
  }, [])

  return { isConnected, refresh, triggerJob, toggleJob }
}

export function useCronJobs() {
  const jobs = useCronStore((state) => state.jobs)
  const filter = useCronStore((state) => state.filter)
  const setFilter = useCronStore((state) => state.setFilter)
  const isLoading = useCronStore((state) => state.isLoading)

  const filteredJobs = useMemo(() => {
    let currentJobs = jobs

    if (filter.status && filter.status !== 'all') {
      currentJobs = currentJobs.filter((j) => j.status === filter.status)
    }

    if (filter.search) {
      const search = filter.search.toLowerCase()
      currentJobs = currentJobs.filter(
        (j) =>
          j.name.toLowerCase().includes(search) ||
          j.description?.toLowerCase().includes(search) ||
          j.cronExpression.includes(search),
      )
    }

    if (filter.flow) {
      currentJobs = currentJobs.filter((j) => j.flows.includes(filter.flow!))
    }

    return currentJobs
  }, [jobs, filter])

  return { jobs: filteredJobs, filter, setFilter, isLoading }
}

export function useJobExecutions(jobId: string | null) {
  const executions = useCronStore((state) => state.executions)

  const streamArgs = useMemo(() => (jobId ? { streamName: 'motia-trace-group', groupId: 'default' } : null), [jobId])

  const { data: traceGroups } = useStreamGroup<TraceGroup & { id: string }>(
    streamArgs || { streamName: '', groupId: '' },
  )

  const jobExecutions = useMemo(() => {
    if (!jobId) return []

    const storeExecutions = executions.filter((exec) => exec.jobId === jobId || exec.jobName === jobId)

    const traceExecutions: CronExecution[] = (traceGroups || [])
      .filter((group) => group.name === jobId || group.name.includes(jobId))
      .map((group) => ({
        id: group.id,
        jobId: group.name,
        jobName: group.name,
        startedAt: group.startTime,
        completedAt: group.endTime,
        duration: group.endTime ? group.endTime - group.startTime : undefined,
        status: group.status,
        traceId: group.id,
      }))

    const allExecutions = [...storeExecutions]
    traceExecutions.forEach((exec) => {
      if (!allExecutions.find((e) => e.id === exec.id)) {
        allExecutions.push(exec)
      }
    })

    return allExecutions.sort((a, b) => b.startedAt - a.startedAt)
  }, [jobId, executions, traceGroups])

  return { executions: jobExecutions }
}

export function useCronStats() {
  const stats = useCronStore((state) => state.stats)
  const jobs = useCronStore((state) => state.jobs)

  const nextJob = useMemo(() => {
    return jobs.filter((j) => j.enabled && j.nextRunAt).sort((a, b) => (a.nextRunAt || 0) - (b.nextRunAt || 0))[0]
  }, [jobs])

  return {
    ...stats,
    nextScheduledJob: nextJob ? { name: nextJob.name, nextRunAt: nextJob.nextRunAt! } : undefined,
  }
}

export const useCronExecutions = useJobExecutions
