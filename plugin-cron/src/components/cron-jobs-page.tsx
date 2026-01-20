import { Badge, Button, cn, Empty, EmptyDescription, EmptyTitle, Input } from '@motiadev/ui'
import {
  Activity,
  AlertCircle,
  Calendar,
  ChevronDown,
  Clock,
  Hash,
  RefreshCw,
  Search,
  Timer,
  Wifi,
  WifiOff,
  X,
} from 'lucide-react'
import type React from 'react'
import { useEffect, useRef, useState } from 'react'
import { useCronJobs, useCronMonitor, useCronStats, useJobExecutions } from '../hooks/use-cron-jobs'
import { useCronStore } from '../stores/cron-store'
import { CronJobCard } from './cron-job-card'

const STATUS_OPTIONS = [
  { value: 'all', label: 'All' },
  { value: 'idle', label: 'Idle' },
  { value: 'running', label: 'Running' },
  { value: 'completed', label: 'Completed' },
  { value: 'failed', label: 'Failed' },
  { value: 'disabled', label: 'Disabled' },
] as const

export const CronJobsPage: React.FC = () => {
  const { isConnected, refresh, triggerJob, toggleJob } = useCronMonitor()
  const { jobs, filter, setFilter, isLoading } = useCronJobs()
  const stats = useCronStats()
  const selectedJobId = useCronStore((state) => state.selectedJobId)
  const selectJob = useCronStore((state) => state.selectJob)
  const { executions } = useJobExecutions(selectedJobId)

  const [showSearch, setShowSearch] = useState(false)
  const [showStatusFilter, setShowStatusFilter] = useState(false)
  const [nextRunCountdown, setNextRunCountdown] = useState<string | null>(null)
  const dropdownRef = useRef<HTMLDivElement>(null)

  // Close dropdown when clicking outside
  useEffect(() => {
    if (!showStatusFilter) return

    const handleClickOutside = (event: MouseEvent) => {
      if (dropdownRef.current && !dropdownRef.current.contains(event.target as Node)) {
        setShowStatusFilter(false)
      }
    }

    document.addEventListener('mousedown', handleClickOutside)
    return () => document.removeEventListener('mousedown', handleClickOutside)
  }, [showStatusFilter])

  // Update countdown for next scheduled job
  useEffect(() => {
    if (!stats.nextScheduledJob) {
      setNextRunCountdown(null)
      return
    }

    const updateCountdown = () => {
      const diff = stats.nextScheduledJob?.nextRunAt - Date.now()
      if (diff <= 0) {
        setNextRunCountdown('Now')
        return
      }

      const seconds = Math.floor(diff / 1000) % 60
      const minutes = Math.floor(diff / 60_000) % 60
      const hours = Math.floor(diff / 3600_000)

      if (hours > 0) {
        setNextRunCountdown(`${hours}h ${minutes}m`)
      } else if (minutes > 0) {
        setNextRunCountdown(`${minutes}m ${seconds}s`)
      } else {
        setNextRunCountdown(`${seconds}s`)
      }
    }

    updateCountdown()
    const interval = setInterval(updateCountdown, 1000)
    return () => clearInterval(interval)
  }, [stats.nextScheduledJob])

  const selectedJob = jobs.find((j) => j.id === selectedJobId)

  return (
    <div className="flex flex-col h-full bg-background text-foreground" role="region" aria-label="Cron Jobs Monitor">
      {/* Header */}
      <div className="flex items-center justify-between px-5 py-3 bg-muted/50 border-b border-border">
        <div className="flex items-center gap-4">
          <h1 className="text-base font-semibold flex items-center gap-2">
            <Clock className="w-4 h-4" />
            Cron Jobs
          </h1>

          <Badge
            variant={isConnected ? 'success' : 'error'}
            className="gap-2"
            aria-label={isConnected ? 'Connected' : 'Disconnected'}
          >
            {isConnected ? <Wifi className="w-3 h-3" /> : <WifiOff className="w-3 h-3" />}
            {isConnected ? 'Live' : 'Offline'}
          </Badge>
        </div>

        <div className="flex items-center gap-2">
          {/* Search Toggle */}
          {showSearch ? (
            <div className="flex items-center gap-2">
              <div className="relative">
                <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-muted-foreground pointer-events-none" />
                <Input
                  type="text"
                  value={filter.search || ''}
                  onChange={(e) => setFilter({ search: e.target.value })}
                  placeholder="Search jobs..."
                  className="w-48 h-7 pl-8 text-xs"
                  aria-label="Search cron jobs"
                  autoFocus
                  onKeyDown={(e) => {
                    if (e.key === 'Escape') {
                      setShowSearch(false)
                      setFilter({ search: '' })
                    }
                  }}
                />
              </div>
              <Button
                variant="ghost"
                size="icon"
                onClick={() => {
                  setShowSearch(false)
                  setFilter({ search: '' })
                }}
                className="h-7 w-7"
                aria-label="Close search"
              >
                <X className="w-3.5 h-3.5" />
              </Button>
            </div>
          ) : (
            <Button
              variant="ghost"
              size="icon"
              onClick={() => setShowSearch(true)}
              className="h-7 w-7 text-muted-foreground hover:text-foreground"
              aria-label="Open search"
            >
              <Search className="w-3.5 h-3.5" />
            </Button>
          )}

          {/* Refresh Button */}
          <Button
            variant="ghost"
            size="sm"
            onClick={refresh}
            disabled={isLoading}
            className="h-7 text-xs text-muted-foreground hover:text-foreground"
            aria-label="Refresh cron jobs"
          >
            <RefreshCw className={cn('w-3.5 h-3.5 mr-1.5', isLoading && 'animate-spin')} />
            Refresh
          </Button>
        </div>
      </div>

      {/* Stats Bar */}
      <div className="px-5 py-3 bg-muted/30 border-b border-border/50">
        <div className="flex items-center gap-6">
          {/* Status Filter */}
          <div className="flex items-center gap-3">
            <span className="text-[11px] font-medium text-muted-foreground uppercase tracking-wide">Status</span>

            <div className="relative" ref={dropdownRef}>
              <Button
                variant={filter.status === 'all' ? 'ghost' : 'default'}
                size="sm"
                onClick={() => setShowStatusFilter(!showStatusFilter)}
                className="h-7 text-xs gap-1.5"
              >
                {STATUS_OPTIONS.find((o) => o.value === filter.status)?.label || 'All'}
                <ChevronDown className="w-3 h-3" />
              </Button>

              {showStatusFilter && (
                <div
                  className="absolute top-full left-0 mt-1 rounded-md shadow-xl py-1 min-w-[120px]"
                  style={{
                    zIndex: 9999,
                    backgroundColor: '#18181b',
                    border: '1px solid #3f3f46',
                  }}
                >
                  {STATUS_OPTIONS.map((option) => (
                    <button
                      key={option.value}
                      onClick={() => {
                        setFilter({ status: option.value })
                        setShowStatusFilter(false)
                      }}
                      className="w-full px-3 py-1.5 text-left text-xs transition-colors"
                      style={{
                        color: filter.status === option.value ? '#ffffff' : '#a1a1aa',
                        backgroundColor: filter.status === option.value ? '#27272a' : 'transparent',
                      }}
                      onMouseEnter={(e) => {
                        e.currentTarget.style.backgroundColor = '#27272a'
                        e.currentTarget.style.color = '#ffffff'
                      }}
                      onMouseLeave={(e) => {
                        e.currentTarget.style.backgroundColor =
                          filter.status === option.value ? '#27272a' : 'transparent'
                        e.currentTarget.style.color = filter.status === option.value ? '#ffffff' : '#a1a1aa'
                      }}
                    >
                      {option.label}
                    </button>
                  ))}
                </div>
              )}
            </div>
          </div>

          {/* Separator */}
          <div className="w-px h-6 bg-border" aria-hidden="true" />

          {/* Quick Stats */}
          <div className="flex items-center gap-4 text-xs">
            <div className="flex items-center gap-1.5 text-muted-foreground" title="Total jobs">
              <Hash className="w-3 h-3" />
              <span className="font-medium text-foreground tabular-nums">{stats.totalJobs}</span>
              jobs
            </div>

            <div className="flex items-center gap-1.5 text-muted-foreground" title="Running">
              <Activity className="w-3 h-3 text-amber-400" />
              <span className="font-medium text-foreground tabular-nums">{stats.runningJobs}</span>
              running
            </div>

            {stats.failedInLast24h > 0 && (
              <div className="flex items-center gap-1.5 text-red-400" title="Failed in last 24h">
                <AlertCircle className="w-3 h-3" />
                <span className="font-medium tabular-nums">{stats.failedInLast24h}</span>
                failed
              </div>
            )}

            {stats.nextScheduledJob && (
              <div
                className="flex items-center gap-1.5 text-muted-foreground"
                title={`Next: ${stats.nextScheduledJob.name}`}
              >
                <Timer className="w-3 h-3 text-emerald-400" />
                <span className="font-medium text-foreground tabular-nums">{nextRunCountdown}</span>
              </div>
            )}
          </div>
        </div>
      </div>

      {/* Main Content */}
      <div className="flex-1 flex overflow-hidden">
        {/* Jobs List */}
        <div className="flex-1 overflow-y-auto p-4 space-y-3">
          {isLoading && jobs.length === 0 ? (
            <div className="flex items-center justify-center h-32">
              <RefreshCw className="w-6 h-6 text-muted-foreground animate-spin" />
            </div>
          ) : jobs.length === 0 ? (
            <Empty className="h-64">
              <div className="w-16 h-16 mb-4 rounded-2xl bg-muted border border-border flex items-center justify-center">
                <Calendar className="w-8 h-8 text-muted-foreground" />
              </div>
              <EmptyTitle>No cron jobs found</EmptyTitle>
              <EmptyDescription>
                {filter.search || filter.status !== 'all'
                  ? 'Try adjusting your filters'
                  : 'Create a cron step to schedule recurring tasks'}
              </EmptyDescription>
            </Empty>
          ) : (
            jobs.map((job) => (
              <CronJobCard
                key={job.id}
                job={job}
                isSelected={selectedJobId === job.id}
                onSelect={() => selectJob(selectedJobId === job.id ? null : job.id)}
                onTrigger={() => triggerJob(job.id)}
                onToggle={(enabled) => toggleJob(job.id, enabled)}
              />
            ))
          )}
        </div>

        {/* Execution Details Panel */}
        {selectedJob && (
          <div className="w-80 border-l border-border bg-muted/30 overflow-y-auto">
            <div className="p-4 border-b border-border">
              <div className="flex items-center justify-between">
                <h2 className="font-medium text-sm">Execution History</h2>
                <Button
                  variant="ghost"
                  size="icon"
                  onClick={() => selectJob(null)}
                  className="h-6 w-6"
                  aria-label="Close panel"
                >
                  <X className="w-3.5 h-3.5" />
                </Button>
              </div>
              <p className="text-xs text-muted-foreground mt-1">{selectedJob.name}</p>
            </div>

            <div className="p-4 space-y-2">
              {executions.length === 0 ? (
                <p className="text-xs text-muted-foreground text-center py-8">No executions yet</p>
              ) : (
                executions.map((exec) => (
                  <div
                    key={exec.id}
                    className={cn(
                      'p-3 rounded-lg border',
                      exec.status === 'running' && 'bg-amber-500/10 border-amber-500/30',
                      exec.status === 'completed' && 'bg-emerald-500/10 border-emerald-500/30',
                      exec.status === 'failed' && 'bg-red-500/10 border-red-500/30',
                    )}
                  >
                    <div className="flex items-center justify-between">
                      <Badge
                        variant={
                          exec.status === 'running' ? 'warning' : exec.status === 'completed' ? 'success' : 'error'
                        }
                        className="text-[10px]"
                      >
                        {exec.status}
                      </Badge>
                      <span className="text-[10px] text-muted-foreground tabular-nums">
                        {new Date(exec.startedAt).toLocaleTimeString()}
                      </span>
                    </div>

                    {exec.duration && (
                      <div className="mt-2 flex items-center gap-1.5 text-xs text-muted-foreground">
                        <Timer className="w-3 h-3" />
                        {exec.duration < 1000 ? `${exec.duration}ms` : `${(exec.duration / 1000).toFixed(2)}s`}
                      </div>
                    )}

                    {exec.error && <p className="mt-2 text-xs text-red-400 line-clamp-2">{exec.error}</p>}

                    {exec.traceId && (
                      <code className="mt-2 block text-[10px] text-muted-foreground font-mono truncate">
                        {exec.traceId}
                      </code>
                    )}
                  </div>
                ))
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  )
}
