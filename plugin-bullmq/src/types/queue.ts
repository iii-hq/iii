import type { Redis, RedisOptions } from 'ioredis'

export type BullMQPluginConfig = {
  connection: Redis | RedisOptions
  prefix?: string
  dlqSuffix?: string
  refreshInterval?: number
}

export type JobStatus = 'waiting' | 'active' | 'completed' | 'failed' | 'delayed' | 'paused' | 'prioritized'

export type QueueStats = {
  waiting: number
  active: number
  completed: number
  failed: number
  delayed: number
  paused: number
  prioritized: number
}

export type QueueInfo = {
  name: string
  displayName: string
  isPaused: boolean
  isDLQ: boolean
  stats: QueueStats
}

export type JobProgress = number | string | object | boolean | null

export type JobInfo = {
  id: string
  name: string
  data: unknown
  opts: Record<string, unknown>
  progress: JobProgress
  attemptsMade: number
  failedReason?: string
  stacktrace?: string[]
  returnvalue?: unknown
  timestamp: number
  finishedOn?: number
  processedOn?: number
  delay?: number
}

export type DLQJobInfo = {
  id: string
  originalEvent: unknown
  failureReason: string
  failureTimestamp: number
  attemptsMade: number
  originalJobId?: string
}

export type CleanOptions = {
  grace: number
  limit: number
  status: JobStatus
}
