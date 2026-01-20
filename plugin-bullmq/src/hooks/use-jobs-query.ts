import { useQuery } from '@tanstack/react-query'
import { useBullMQStore } from '../stores/use-bullmq-store'
import type { DLQJobInfo, JobInfo, JobStatus } from '../types/queue'

const fetchJobs = async (queueName: string, status: JobStatus, start = 0, end = 100): Promise<JobInfo[]> => {
  const params = new URLSearchParams({ status, start: String(start), end: String(end) })
  const response = await fetch(`/__motia/bullmq/queues/${encodeURIComponent(queueName)}/jobs?${params}`)
  if (!response.ok) {
    throw new Error('Failed to fetch jobs')
  }
  const data = await response.json()
  return data.jobs
}

const fetchJob = async (queueName: string, jobId: string): Promise<JobInfo | null> => {
  const response = await fetch(
    `/__motia/bullmq/queues/${encodeURIComponent(queueName)}/jobs/${encodeURIComponent(jobId)}`,
  )
  if (!response.ok) {
    return null
  }
  return response.json()
}

const fetchDLQJobs = async (queueName: string, start = 0, end = 100): Promise<DLQJobInfo[]> => {
  const params = new URLSearchParams({ start: String(start), end: String(end) })
  const response = await fetch(`/__motia/bullmq/dlq/${encodeURIComponent(queueName)}/jobs?${params}`)
  if (!response.ok) {
    return []
  }
  const data = await response.json()
  return data.jobs
}

export const useJobsQuery = () => {
  const selectedQueue = useBullMQStore((state) => state.selectedQueue)
  const selectedStatus = useBullMQStore((state) => state.selectedStatus)

  const queueName = selectedQueue?.name
  const statsKey = selectedQueue ? JSON.stringify(selectedQueue.stats) : null

  return useQuery<JobInfo[]>({
    queryKey: ['jobs', queueName, selectedStatus, statsKey],
    queryFn: () => fetchJobs(queueName!, selectedStatus),
    enabled: !!queueName,
    staleTime: 5000,
  })
}

export const useJobQuery = (queueName: string | undefined, jobId: string | undefined) => {
  return useQuery<JobInfo | null>({
    queryKey: ['job', queueName, jobId],
    queryFn: () => fetchJob(queueName!, jobId!),
    enabled: !!queueName && !!jobId,
  })
}

export const useDLQJobsQuery = (queueName: string | undefined) => {
  return useQuery<DLQJobInfo[]>({
    queryKey: ['dlq-jobs', queueName],
    queryFn: () => fetchDLQJobs(queueName!),
    enabled: !!queueName,
  })
}
