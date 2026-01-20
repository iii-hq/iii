export type CronJobStatus = 'idle' | 'running' | 'completed' | 'failed' | 'disabled'

export interface CronJob {
  /** Unique identifier for the cron job */
  id: string
  /** Step name */
  name: string
  /** Description from step config */
  description?: string
  /** Cron expression (e.g., "0 * * * *") */
  cronExpression: string
  /** Human-readable cron description */
  cronDescription?: string
  /** Current status */
  status: CronJobStatus
  /** Associated flows */
  flows: string[]
  /** File path to the step */
  filePath?: string
  /** Next scheduled execution time */
  nextRunAt?: number
  /** Last execution time */
  lastRunAt?: number
  /** Last execution duration in ms */
  lastDuration?: number
  /** Total execution count */
  runCount: number
  /** Error count */
  errorCount: number
  /** Last error message */
  lastError?: string
  /** Is the job enabled */
  enabled: boolean
  /** Timezone for the cron expression */
  timezone?: string
  /** Created timestamp */
  createdAt: number
}

export interface CronExecution {
  /** Execution ID */
  id: string
  /** Job ID */
  jobId: string
  /** Job name */
  jobName: string
  /** Start time */
  startedAt: number
  /** End time */
  completedAt?: number
  /** Duration in ms */
  duration?: number
  /** Status */
  status: 'running' | 'completed' | 'failed'
  /** Error message if failed */
  error?: string
  /** Trace ID for correlation */
  traceId?: string
}

export interface CronStats {
  /** Total number of cron jobs */
  totalJobs: number
  /** Currently running jobs */
  runningJobs: number
  /** Failed jobs in last 24h */
  failedInLast24h: number
  /** Total executions today */
  executionsToday: number
  /** Average execution time */
  avgExecutionTime: number
  /** Next scheduled job */
  nextScheduledJob?: {
    name: string
    nextRunAt: number
  }
}

export interface CronFilter {
  status?: CronJobStatus | 'all'
  search?: string
  flow?: string
}

/** Log message from Motia logs stream */
export interface CronLogMessage {
  traceId: string
  stepName?: string
  message?: string
  level?: string
  timestamp: number
  [key: string]: unknown
}
