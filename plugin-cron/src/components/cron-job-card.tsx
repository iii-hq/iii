import { Badge, Button, cn } from '@motiadev/ui'
import {
  AlertCircle,
  Calendar,
  CheckCircle2,
  ChevronRight,
  Clock,
  Hash,
  Loader2,
  Pause,
  Play,
  Timer,
} from 'lucide-react'
import type React from 'react'
import type { CronJob } from '../types/cron'

interface CronJobCardProps {
  job: CronJob
  isSelected: boolean
  onSelect: () => void
  onTrigger: () => void
  onToggle: (enabled: boolean) => void
}

const statusConfig: Record<
  CronJob['status'],
  { variant: 'success' | 'warning' | 'error' | 'default' | 'info'; icon: React.ReactNode; label: string }
> = {
  idle: {
    variant: 'info',
    icon: <Clock className="w-3 h-3" />,
    label: 'Idle',
  },
  running: {
    variant: 'warning',
    icon: <Loader2 className="w-3 h-3 animate-spin" />,
    label: 'Running',
  },
  completed: {
    variant: 'success',
    icon: <CheckCircle2 className="w-3 h-3" />,
    label: 'Completed',
  },
  failed: {
    variant: 'error',
    icon: <AlertCircle className="w-3 h-3" />,
    label: 'Failed',
  },
  disabled: {
    variant: 'default',
    icon: <Pause className="w-3 h-3" />,
    label: 'Disabled',
  },
}

function formatRelativeTime(timestamp: number | undefined): string {
  if (!timestamp) return 'Never'

  const now = Date.now()
  const diff = timestamp - now
  const absDiff = Math.abs(diff)

  if (absDiff < 60_000) {
    const seconds = Math.round(absDiff / 1000)
    return diff > 0 ? `in ${seconds}s` : `${seconds}s ago`
  }

  if (absDiff < 3600_000) {
    const minutes = Math.round(absDiff / 60_000)
    return diff > 0 ? `in ${minutes}m` : `${minutes}m ago`
  }

  if (absDiff < 86400_000) {
    const hours = Math.round(absDiff / 3600_000)
    return diff > 0 ? `in ${hours}h` : `${hours}h ago`
  }

  const days = Math.round(absDiff / 86400_000)
  return diff > 0 ? `in ${days}d` : `${days}d ago`
}

function formatDuration(ms: number | undefined): string {
  if (!ms) return '-'
  if (ms < 1000) return `${ms}ms`
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`
  return `${(ms / 60_000).toFixed(1)}m`
}

export const CronJobCard: React.FC<CronJobCardProps> = ({ job, isSelected, onSelect, onTrigger, onToggle }) => {
  const status = statusConfig[job.status]

  return (
    <div
      className={cn(
        'group relative rounded-lg border transition-all duration-200 cursor-pointer',
        'hover:shadow-md hover:border-primary/50',
        isSelected
          ? 'bg-primary/10 border-primary shadow-sm ring-1 ring-primary/30'
          : 'bg-card border-border hover:bg-muted/50',
        !job.enabled && 'opacity-60',
      )}
      onClick={onSelect}
      role="option"
      tabIndex={0}
      aria-selected={isSelected}
      aria-label={`Cron job: ${job.name}`}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault()
          onSelect()
        }
      }}
    >
      <div className="p-4">
        {/* Header */}
        <div className="flex items-start justify-between gap-3">
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2">
              <h3 className="font-medium text-foreground truncate">{job.name}</h3>
              <Badge variant={status.variant} className="gap-1 shrink-0">
                {status.icon}
                {status.label}
              </Badge>
            </div>

            {job.description && <p className="text-xs text-muted-foreground mt-1 line-clamp-1">{job.description}</p>}
          </div>

          <div className="flex items-center gap-1 shrink-0">
            <Button
              variant="ghost"
              size="icon"
              onClick={(e) => {
                e.stopPropagation()
                onTrigger()
              }}
              disabled={job.status === 'running' || !job.enabled}
              className="h-7 w-7 text-muted-foreground hover:text-foreground"
              aria-label="Trigger job manually"
            >
              <Play className="w-3.5 h-3.5" />
            </Button>

            <Button
              variant="ghost"
              size="icon"
              onClick={(e) => {
                e.stopPropagation()
                onToggle(!job.enabled)
              }}
              className={cn(
                'h-7 w-7',
                job.enabled ? 'text-muted-foreground hover:text-foreground' : 'text-amber-500 hover:text-amber-400',
              )}
              aria-label={job.enabled ? 'Disable job' : 'Enable job'}
            >
              {job.enabled ? <Pause className="w-3.5 h-3.5" /> : <Play className="w-3.5 h-3.5" />}
            </Button>

            <ChevronRight
              className={cn('w-4 h-4 text-muted-foreground transition-transform', isSelected && 'rotate-90')}
            />
          </div>
        </div>

        {/* Cron Expression */}
        <div className="mt-3 flex items-center gap-2">
          <code className="px-2 py-1 rounded bg-muted text-xs font-mono text-muted-foreground">
            {job.cronExpression}
          </code>
          <span className="text-xs text-muted-foreground truncate">{job.cronDescription}</span>
        </div>

        {/* Stats Row */}
        <div className="mt-3 flex items-center gap-4 text-xs text-muted-foreground">
          <div className="flex items-center gap-1.5" title="Next run">
            <Calendar className="w-3 h-3" />
            <span className="tabular-nums">{job.enabled ? formatRelativeTime(job.nextRunAt) : 'Disabled'}</span>
          </div>

          <div className="flex items-center gap-1.5" title="Last duration">
            <Timer className="w-3 h-3" />
            <span className="tabular-nums">{formatDuration(job.lastDuration)}</span>
          </div>

          <div className="flex items-center gap-1.5" title="Total runs">
            <Hash className="w-3 h-3" />
            <span className="tabular-nums">{job.runCount}</span>
          </div>

          {job.errorCount > 0 && (
            <div className="flex items-center gap-1.5 text-red-400" title="Errors">
              <AlertCircle className="w-3 h-3" />
              <span className="tabular-nums">{job.errorCount}</span>
            </div>
          )}
        </div>

        {/* Error Message */}
        {job.lastError && job.status === 'failed' && (
          <div className="mt-2 p-2 rounded bg-red-500/10 border border-red-500/20">
            <p className="text-xs text-red-400 line-clamp-2">{job.lastError}</p>
          </div>
        )}

        {/* Flows */}
        {job.flows.length > 0 && (
          <div className="mt-3 flex items-center gap-2 flex-wrap">
            {job.flows.map((flow) => (
              <span key={flow} className="px-2 py-0.5 rounded-full bg-primary/10 text-primary text-[10px] font-medium">
                {flow}
              </span>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
