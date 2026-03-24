import { useQuery } from '@tanstack/react-query'
import { createFileRoute } from '@tanstack/react-router'
import {
  Check,
  Clock,
  Copy,
  CornerDownRight,
  Download,
  ExternalLink,
  Filter,
  Pause,
  Play,
  Search,
  Terminal,
  Trash2,
  X,
  Zap,
} from 'lucide-react'
import { useEffect, useMemo, useReducer, useRef, useState } from 'react'
import type { OtelLog } from '@/api'
import { otelLogsQuery } from '@/api/queries'
import { type TimeRange, TimeRangeFilter } from '@/components/filters/TimeRangeFilter'
import { Badge, Button, Input } from '@/components/ui/card'
import { EmptyState } from '@/components/ui/empty-state'
import { JsonViewer } from '@/components/ui/json-viewer'
import { Pagination } from '@/components/ui/pagination'
import { Skeleton } from '@/components/ui/skeleton'

export const Route = createFileRoute('/logs')({
  component: LogsPage,
})

// --- Filter Reducer ---
interface FilterState {
  searchQuery: string
  activeLevelFilters: Set<string>
  timeRange: TimeRange | undefined
  selectedSeverity: string
}

type FilterAction =
  | { type: 'SET_SEARCH_QUERY'; payload: string }
  | { type: 'TOGGLE_LEVEL_FILTER'; payload: string }
  | { type: 'CLEAR_LEVEL_FILTERS' }
  | { type: 'SET_TIME_RANGE'; payload: TimeRange | undefined }
  | { type: 'SET_SEVERITY'; payload: string }
  | { type: 'CLEAR_TIME_AND_SEVERITY' }

function filterReducer(state: FilterState, action: FilterAction): FilterState {
  switch (action.type) {
    case 'SET_SEARCH_QUERY':
      return { ...state, searchQuery: action.payload }
    case 'TOGGLE_LEVEL_FILTER': {
      const next = new Set(state.activeLevelFilters)
      if (next.has(action.payload)) {
        next.delete(action.payload)
      } else {
        next.add(action.payload)
      }
      return { ...state, activeLevelFilters: next }
    }
    case 'CLEAR_LEVEL_FILTERS':
      return { ...state, activeLevelFilters: new Set() }
    case 'SET_TIME_RANGE':
      return { ...state, timeRange: action.payload }
    case 'SET_SEVERITY':
      return { ...state, selectedSeverity: action.payload }
    case 'CLEAR_TIME_AND_SEVERITY':
      return { ...state, timeRange: undefined, selectedSeverity: '' }
    default:
      return state
  }
}

// --- Pagination Reducer ---
interface PaginationState {
  currentPage: number
  pageSize: number
}

type PaginationAction =
  | { type: 'SET_PAGE'; payload: number }
  | { type: 'SET_PAGE_SIZE'; payload: number }

function paginationReducer(state: PaginationState, action: PaginationAction): PaginationState {
  switch (action.type) {
    case 'SET_PAGE':
      return { ...state, currentPage: action.payload }
    case 'SET_PAGE_SIZE':
      return { ...state, pageSize: action.payload }
    default:
      return state
  }
}

// --- UI Reducer ---
interface UiState {
  selectedLogId: string | undefined
  fullscreenLogId: string | null
  copied: string | null
}

type UiAction =
  | { type: 'SET_SELECTED_LOG_ID'; payload: string | undefined }
  | { type: 'SET_FULLSCREEN_LOG_ID'; payload: string | null }
  | { type: 'SET_COPIED'; payload: string | null }
  | { type: 'CLEAR_SELECTED_LOG' }

function uiReducer(state: UiState, action: UiAction): UiState {
  switch (action.type) {
    case 'SET_SELECTED_LOG_ID':
      return { ...state, selectedLogId: action.payload }
    case 'SET_FULLSCREEN_LOG_ID':
      return { ...state, fullscreenLogId: action.payload }
    case 'SET_COPIED':
      return { ...state, copied: action.payload }
    case 'CLEAR_SELECTED_LOG':
      return { ...state, selectedLogId: undefined }
    default:
      return state
  }
}

interface LogEntry {
  id: string
  timestamp: string
  time: number
  level: 'info' | 'warn' | 'error' | 'debug'
  message: string
  source: string
  traceId?: string
  context?: Record<string, unknown>
}

const LEVEL_CONFIG: Record<
  string,
  {
    dot: string
    text: string
    bg: string
    badge: string
    label: string
  }
> = {
  debug: {
    dot: 'bg-gray-400',
    text: 'text-gray-400',
    bg: 'hover:bg-gray-500/10',
    badge: 'bg-gray-500/20 text-gray-400 border-gray-500/30',
    label: 'DEBUG',
  },
  info: {
    dot: 'bg-cyan-400',
    text: 'text-cyan-400',
    bg: 'hover:bg-cyan-500/10',
    badge: 'bg-cyan-500/20 text-cyan-400 border-cyan-500/30',
    label: 'INFO',
  },
  warn: {
    dot: 'bg-yellow',
    text: 'text-yellow',
    bg: 'hover:bg-yellow/10',
    badge: 'bg-yellow/20 text-yellow border-yellow/30',
    label: 'WARN',
  },
  error: {
    dot: 'bg-red-400',
    text: 'text-red-400',
    bg: 'hover:bg-red-500/10',
    badge: 'bg-red-500/20 text-red-400 border-red-500/30',
    label: 'ERROR',
  },
}

function formatTimestamp(time: number | string): string {
  const date = typeof time === 'number' ? new Date(time) : new Date(time)
  return date.toLocaleTimeString('en-US', {
    hour12: false,
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    fractionalSecondDigits: 3,
  })
}

function formatFullTimestamp(time: number | string): string {
  const date = typeof time === 'number' ? new Date(time) : new Date(time)
  return date.toISOString()
}

function LogsPage() {
  const [clearedData, setClearedData] = useState<unknown>(null)
  const [isPaused, setIsPaused] = useState(false)
  const autoScroll = true

  const [filterState, dispatchFilter] = useReducer(filterReducer, {
    searchQuery: '',
    activeLevelFilters: new Set<string>(),
    timeRange: undefined,
    selectedSeverity: '',
  })
  const { searchQuery, activeLevelFilters, timeRange, selectedSeverity } = filterState

  const [pagination, dispatchPagination] = useReducer(paginationReducer, {
    currentPage: 1,
    pageSize: 50,
  })
  const { currentPage, pageSize } = pagination

  const [uiState, dispatchUi] = useReducer(uiReducer, {
    selectedLogId: undefined,
    fullscreenLogId: null,
    copied: null,
  })
  const { selectedLogId, fullscreenLogId, copied } = uiState

  const logContainerRef = useRef<HTMLDivElement>(null)
  const lastLogCountRef = useRef(0)

  const { data: otelLogsData, isLoading } = useQuery({
    ...otelLogsQuery({
      start_time: timeRange?.startTime,
      end_time: timeRange?.endTime,
      severity_text: selectedSeverity || undefined,
      limit: 500,
      offset: 0,
    }),
    refetchInterval: isPaused ? false : 2000,
    staleTime: 1000, // Data is fresh for 1s to prevent race conditions with refetch interval
  })

  const hasLoggingAdapter = useMemo(() => {
    if (!otelLogsData) return false
    if (otelLogsData.logs && otelLogsData.logs.length > 0) return true
    return otelLogsData.logs !== undefined
  }, [otelLogsData])

  const logs = useMemo(() => {
    if (!otelLogsData?.logs || otelLogsData.logs.length === 0) return []
    if (otelLogsData === clearedData) return []
    const levelMap: Record<string, LogEntry['level']> = {
      DEBUG: 'debug',
      INFO: 'info',
      WARN: 'warn',
      WARNING: 'warn',
      ERROR: 'error',
    }
    return otelLogsData.logs.map((log: OtelLog, i: number) => {
      const serviceName = (log.resource?.['service.name'] as string) || 'unknown'
      const level = levelMap[log.severity_text?.toUpperCase()] || 'info'
      const timestampMs = log.timestamp_unix_nano / 1_000_000
      return {
        id: `${log.trace_id || log.timestamp_unix_nano}-${i}`,
        timestamp: new Date(timestampMs).toISOString(),
        time: timestampMs,
        level,
        message: log.body,
        source: serviceName,
        traceId: log.trace_id || undefined,
        context: log.attributes as Record<string, unknown>,
      }
    })
  }, [otelLogsData, clearedData])

  useEffect(() => {
    if (logs.length > lastLogCountRef.current && autoScroll && logContainerRef.current) {
      setTimeout(() => {
        logContainerRef.current?.scrollTo({ top: 0, behavior: 'smooth' })
      }, 100)
    }
    lastLogCountRef.current = logs.length
  }, [logs])

  // Handle Escape key to close fullscreen modal
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && fullscreenLogId) {
        dispatchUi({ type: 'SET_FULLSCREEN_LOG_ID', payload: null })
      }
    }

    document.addEventListener('keydown', handleKeyDown)
    return () => document.removeEventListener('keydown', handleKeyDown)
  }, [fullscreenLogId])

  const filteredLogs = useMemo(() => {
    return logs.filter((log) => {
      if (activeLevelFilters.size > 0 && !activeLevelFilters.has(log.level)) {
        return false
      }

      if (searchQuery) {
        const searchLower = searchQuery.toLowerCase()
        return (
          log.message?.toLowerCase().includes(searchLower) ||
          log.traceId?.toLowerCase().includes(searchLower) ||
          log.source?.toLowerCase().includes(searchLower)
        )
      }

      return true
    })
  }, [logs, searchQuery, activeLevelFilters])

  // Pagination
  const totalPages = Math.max(1, Math.ceil(filteredLogs.length / pageSize))
  const paginatedLogs = useMemo(() => {
    const start = (currentPage - 1) * pageSize
    return filteredLogs.slice(start, start + pageSize)
  }, [filteredLogs, currentPage, pageSize])

  const selectedLog = useMemo(() => {
    return selectedLogId ? logs.find((log) => log.id === selectedLogId) : undefined
  }, [logs, selectedLogId])

  const fullscreenLog = useMemo(() => {
    return fullscreenLogId ? logs.find((log) => log.id === fullscreenLogId) : undefined
  }, [logs, fullscreenLogId])

  const clearLogs = () => {
    setClearedData(otelLogsData)
    dispatchUi({ type: 'CLEAR_SELECTED_LOG' })
    lastLogCountRef.current = 0
  }

  const toggleLevelFilter = (level: string) => {
    dispatchFilter({ type: 'TOGGLE_LEVEL_FILTER', payload: level })
  }

  const filterByTrace = (traceId: string) => {
    dispatchFilter({ type: 'SET_SEARCH_QUERY', payload: traceId })
  }

  const copyToClipboard = (text: string, key: string) => {
    navigator.clipboard.writeText(text)
    dispatchUi({ type: 'SET_COPIED', payload: key })
    setTimeout(() => dispatchUi({ type: 'SET_COPIED', payload: null }), 2000)
  }

  const exportLogs = () => {
    const exportData = filteredLogs.map((log) => ({
      timestamp: log.timestamp,
      level: log.level,
      source: log.source,
      message: log.message,
      traceId: log.traceId,
      context: log.context,
    }))
    const blob = new Blob([JSON.stringify(exportData, null, 2)], {
      type: 'application/json',
    })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = `iii-logs-${new Date().toISOString().slice(0, 10)}.json`
    a.click()
    URL.revokeObjectURL(url)
  }

  const levelCounts = useMemo(
    () => ({
      info: logs.filter((l) => l.level === 'info').length,
      warn: logs.filter((l) => l.level === 'warn').length,
      error: logs.filter((l) => l.level === 'error').length,
      debug: logs.filter((l) => l.level === 'debug').length,
    }),
    [logs],
  )

  const sourceCounts = useMemo(() => {
    const counts: Record<string, number> = {}
    logs.forEach((log) => {
      counts[log.source] = (counts[log.source] || 0) + 1
    })
    return Object.entries(counts)
      .sort((a, b) => b[1] - a[1])
      .slice(0, 5)
  }, [logs])

  return (
    <div className="flex flex-col h-full bg-background text-foreground">
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 px-3 md:px-5 py-3 md:py-4 bg-dark-gray/30 border-b border-border">
        <div className="flex items-center gap-2 md:gap-4 flex-wrap">
          <h1 className="font-sans font-semibold text-lg tracking-tight flex items-center gap-2">
            <Terminal className="w-5 h-5" />
            Logs
          </h1>
          {isPaused && (
            <Badge variant="warning" className="gap-1 text-[10px] md:text-xs">
              <Pause className="w-2.5 h-2.5 md:w-3 md:h-3" />
              <span className="hidden sm:inline">Paused</span>
            </Badge>
          )}
          {logs.length > 0 && (
            <span className="font-sans text-sm text-secondary">
              {filteredLogs.length}/{logs.length}
            </span>
          )}
        </div>

        <div className="flex items-center gap-1.5 md:gap-2">
          <Button
            variant={isPaused ? 'accent' : 'ghost'}
            size="sm"
            onClick={() => setIsPaused(!isPaused)}
            disabled={!hasLoggingAdapter}
            className="h-6 md:h-7 text-[10px] md:text-xs px-2"
          >
            {isPaused ? (
              <Play className="w-3 h-3 md:mr-1.5" />
            ) : (
              <Pause className="w-3 h-3 md:mr-1.5" />
            )}
            <span className="hidden md:inline">{isPaused ? 'Resume' : 'Pause'}</span>
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={exportLogs}
            disabled={filteredLogs.length === 0}
            className="h-6 md:h-7 text-[10px] md:text-xs px-2"
          >
            <Download className="w-3 h-3 md:mr-1.5" />
            <span className="hidden md:inline">Export</span>
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={clearLogs}
            disabled={logs.length === 0}
            className="h-6 md:h-7 text-[10px] md:text-xs px-2"
          >
            <Trash2 className="w-3 h-3 md:mr-1.5" />
            <span className="hidden md:inline">Clear</span>
          </Button>
        </div>
      </div>

      {isLoading && (
        <div className="flex-1 p-4 space-y-2">
          {(
            [
              'log-sk-0',
              'log-sk-1',
              'log-sk-2',
              'log-sk-3',
              'log-sk-4',
              'log-sk-5',
              'log-sk-6',
              'log-sk-7',
            ] as const
          ).map((sk) => (
            <Skeleton key={sk} className="h-9 w-full" />
          ))}
        </div>
      )}

      {!isLoading && !hasLoggingAdapter && (
        <div className="flex-1 flex items-center justify-center p-12">
          <EmptyState
            icon={Terminal}
            title="No logs yet"
            description="Logs will appear here as your functions run"
          />
        </div>
      )}

      {hasLoggingAdapter && (
        <>
          <div className="flex flex-col gap-2 p-2 border-b border-border bg-dark-gray/20">
            {/* Time Range and Severity Filters */}
            <div className="flex items-center gap-2 flex-wrap">
              <TimeRangeFilter
                value={timeRange}
                onChange={(v) => dispatchFilter({ type: 'SET_TIME_RANGE', payload: v })}
                compactMode
              />

              <select
                value={selectedSeverity}
                onChange={(e) => dispatchFilter({ type: 'SET_SEVERITY', payload: e.target.value })}
                className="px-3 py-1.5 rounded-[var(--radius-md)] text-xs bg-border-subtle border border-border-subtle hover:border-muted text-foreground"
              >
                <option value="">All Severities</option>
                <option value="DEBUG">DEBUG</option>
                <option value="INFO">INFO</option>
                <option value="WARN">WARN</option>
                <option value="ERROR">ERROR</option>
              </select>

              {(timeRange || selectedSeverity) && (
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => dispatchFilter({ type: 'CLEAR_TIME_AND_SEVERITY' })}
                  className="h-7 text-xs px-2"
                >
                  <X className="w-3 h-3 mr-1" />
                  Clear Filters
                </Button>
              )}
            </div>

            {/* Search Bar */}
            <div className="flex items-center gap-2">
              <div className="flex-1 relative">
                <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted" />
                <Input
                  value={searchQuery}
                  onChange={(e) =>
                    dispatchFilter({ type: 'SET_SEARCH_QUERY', payload: e.target.value })
                  }
                  className="pl-9 pr-9 h-9 font-medium"
                  placeholder="Search by Trace ID, Source, or Message..."
                />
                {searchQuery && (
                  <button
                    type="button"
                    onClick={() => dispatchFilter({ type: 'SET_SEARCH_QUERY', payload: '' })}
                    className="absolute right-3 top-1/2 -translate-y-1/2 text-muted hover:text-foreground"
                  >
                    <X className="w-4 h-4" />
                  </button>
                )}
              </div>

              <div className="flex items-center gap-1 px-2 border-l border-border">
                {(['info', 'warn', 'error', 'debug'] as const).map((level) => {
                  const config = LEVEL_CONFIG[level]
                  const isActive = activeLevelFilters.has(level)
                  const count = levelCounts[level]

                  return (
                    <button
                      key={level}
                      type="button"
                      onClick={() => toggleLevelFilter(level)}
                      className={`flex items-center gap-1.5 px-2.5 py-1.5 rounded text-xs transition-all ${
                        isActive
                          ? `${config.badge} border`
                          : 'hover:bg-dark-gray/50 border border-transparent'
                      }`}
                      title={`${isActive ? 'Hide' : 'Show only'} ${level} logs`}
                    >
                      <span className={`w-2 h-2 rounded-full ${config.dot}`} />
                      <span
                        className={`font-medium tabular-nums ${isActive ? config.text : 'text-muted'}`}
                      >
                        {count}
                      </span>
                    </button>
                  )
                })}

                {activeLevelFilters.size > 0 && (
                  <button
                    type="button"
                    onClick={() => dispatchFilter({ type: 'CLEAR_LEVEL_FILTERS' })}
                    className="ml-1 p-1 rounded hover:bg-dark-gray/50 text-muted hover:text-foreground transition-colors"
                    title="Clear level filters"
                  >
                    <X className="w-3.5 h-3.5" />
                  </button>
                )}
              </div>
            </div>
          </div>

          <div className="flex-1 flex overflow-hidden">
            <div className="flex flex-col flex-1 overflow-hidden">
              <div ref={logContainerRef} className="flex-1 overflow-auto">
                <div className="relative w-full">
                  {filteredLogs.length === 0 ? (
                    <EmptyState
                      icon={Terminal}
                      title={
                        searchQuery || activeLevelFilters.size > 0
                          ? 'No logs match your filters'
                          : 'No logs yet'
                      }
                      description={
                        searchQuery || activeLevelFilters.size > 0
                          ? 'Try adjusting your search or level filters'
                          : 'Logs will appear here as your functions run'
                      }
                      action={
                        searchQuery || activeLevelFilters.size > 0
                          ? {
                              label: 'Clear all filters',
                              onClick: () => {
                                dispatchFilter({ type: 'SET_SEARCH_QUERY', payload: '' })
                                dispatchFilter({ type: 'CLEAR_LEVEL_FILTERS' })
                              },
                            }
                          : undefined
                      }
                      className="h-64"
                    />
                  ) : (
                    paginatedLogs.map((log) => {
                      const config = LEVEL_CONFIG[log.level] || LEVEL_CONFIG.info
                      const isSelected = selectedLogId === log.id

                      return (
                        <button
                          key={log.id}
                          type="button"
                          className={`flex items-center font-mono cursor-pointer text-[13px] h-9 px-3 border-b border-border/20 transition-colors w-full text-left
                            ${isSelected ? 'bg-primary/10 border-l-2 border-l-primary' : `${config.bg} border-l-2 border-l-transparent`}
                          `}
                          onClick={() =>
                            dispatchUi({
                              type: 'SET_SELECTED_LOG_ID',
                              payload: isSelected ? undefined : log.id,
                            })
                          }
                          onDoubleClick={() =>
                            dispatchUi({ type: 'SET_FULLSCREEN_LOG_ID', payload: log.id })
                          }
                          title="Click to select, double-click for fullscreen"
                        >
                          <div className="flex items-center gap-2 text-muted shrink-0 w-[100px]">
                            <span className={`w-2 h-2 rounded-full shrink-0 ${config.dot}`} />
                            <span className="text-xs">{formatTimestamp(log.time)}</span>
                          </div>

                          {log.traceId && (
                            <div className="flex items-center shrink-0 ml-2">
                              <span className="text-muted font-mono text-[11px] bg-black/30 px-1.5 py-0.5 rounded">
                                {log.traceId.slice(0, 8)}
                              </span>
                              <button
                                type="button"
                                onClick={(e) => {
                                  e.stopPropagation()
                                  filterByTrace(log.traceId ?? '')
                                }}
                                className="p-1 rounded hover:bg-dark-gray/50 text-muted hover:text-primary transition-colors ml-0.5"
                                title={`Filter by trace ${log.traceId}`}
                              >
                                <Filter className="w-3 h-3" />
                              </button>
                            </div>
                          )}

                          <div className="text-muted shrink-0 ml-3 px-2 py-0.5 bg-dark-gray/50 rounded text-[11px] max-w-[180px] truncate">
                            {log.source}
                          </div>

                          <div className={`ml-3 flex-1 truncate ${config.text}`}>{log.message}</div>

                          {log.context && Object.keys(log.context).length > 0 && (
                            <div className="shrink-0 ml-2">
                              <span className="text-[10px] text-muted bg-dark-gray/50 px-1.5 py-0.5 rounded">
                                +{Object.keys(log.context).length}
                              </span>
                            </div>
                          )}
                        </button>
                      )
                    })
                  )}
                </div>
              </div>

              {/* Pagination */}
              {filteredLogs.length > 0 && (
                <div className="flex-shrink-0 bg-background/95 backdrop-blur border-t border-border px-3 py-2">
                  <Pagination
                    currentPage={currentPage}
                    totalPages={totalPages}
                    totalItems={filteredLogs.length}
                    pageSize={pageSize}
                    onPageChange={(p) => dispatchPagination({ type: 'SET_PAGE', payload: p })}
                    onPageSizeChange={(s) =>
                      dispatchPagination({ type: 'SET_PAGE_SIZE', payload: s })
                    }
                    pageSizeOptions={[25, 50, 100, 250, 500]}
                  />
                </div>
              )}
            </div>

            {selectedLog && (
              <div className="w-[400px] border-l border-border bg-dark-gray/20 overflow-y-auto">
                <div className="p-4 border-b border-border sticky top-0 bg-dark-gray/80 backdrop-blur z-10">
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <span
                        className={`px-2 py-0.5 rounded text-[10px] font-semibold uppercase ${LEVEL_CONFIG[selectedLog.level]?.badge}`}
                      >
                        {selectedLog.level}
                      </span>
                    </div>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => dispatchUi({ type: 'CLEAR_SELECTED_LOG' })}
                      className="h-6 w-6 p-0"
                    >
                      <X className="w-3.5 h-3.5" />
                    </Button>
                  </div>
                  <p className="text-xs text-muted mt-2 font-mono">
                    {formatFullTimestamp(selectedLog.time)}
                  </p>
                </div>

                <div className="p-4 space-y-5">
                  <div>
                    <div className="font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted mb-2 flex items-center gap-2">
                      Message
                      <button
                        type="button"
                        onClick={() => copyToClipboard(selectedLog.message, 'message')}
                        className="p-0.5 rounded hover:bg-dark-gray/50 transition-colors"
                      >
                        {copied === 'message' ? (
                          <Check className="w-3 h-3 text-success" />
                        ) : (
                          <Copy className="w-3 h-3" />
                        )}
                      </button>
                    </div>
                    <p
                      className={`text-sm leading-relaxed ${LEVEL_CONFIG[selectedLog.level]?.text}`}
                    >
                      {selectedLog.message}
                    </p>
                  </div>

                  {selectedLog.traceId && (
                    <div>
                      <div className="font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted mb-2 flex items-center gap-2">
                        Trace ID
                        <button
                          type="button"
                          onClick={() => copyToClipboard(selectedLog.traceId ?? '', 'traceId')}
                          className="p-0.5 rounded hover:bg-dark-gray/50 transition-colors"
                        >
                          {copied === 'traceId' ? (
                            <Check className="w-3 h-3 text-success" />
                          ) : (
                            <Copy className="w-3 h-3" />
                          )}
                        </button>
                      </div>
                      <div className="flex items-center gap-2">
                        <code className="font-mono text-[13px] bg-black/40 px-2 py-1.5 rounded flex-1 break-all">
                          {selectedLog.traceId}
                        </code>
                        <button
                          type="button"
                          onClick={() => filterByTrace(selectedLog.traceId ?? '')}
                          className="p-1.5 rounded bg-primary/20 hover:bg-primary/30 text-primary transition-colors"
                          title="Filter by this trace"
                        >
                          <Filter className="w-3.5 h-3.5" />
                        </button>
                      </div>
                    </div>
                  )}

                  <div>
                    <div className="font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted mb-2">
                      Source Function
                    </div>
                    <div className="flex items-center gap-2">
                      <code className="font-mono text-[13px] bg-cyan-500/10 text-cyan-400 px-2 py-1.5 rounded border border-cyan-500/20">
                        <Zap className="w-3 h-3 inline mr-1.5" />
                        {selectedLog.source}
                      </code>
                    </div>
                  </div>

                  <div>
                    <div className="font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted mb-2">
                      Timestamp
                    </div>
                    <div className="grid grid-cols-2 gap-2">
                      <div className="bg-elevated rounded-[var(--radius-md)] px-2 py-1.5">
                        <div className="font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted">
                          ISO
                        </div>
                        <code className="font-mono text-[13px]">{selectedLog.timestamp}</code>
                      </div>
                      <div className="bg-elevated rounded-[var(--radius-md)] px-2 py-1.5">
                        <div className="font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted">
                          Unix
                        </div>
                        <code className="font-mono text-[13px]">{selectedLog.time}</code>
                      </div>
                    </div>
                  </div>

                  {selectedLog.context && Object.keys(selectedLog.context).length > 0 && (
                    <div>
                      <div className="font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted mb-2 flex items-center gap-2">
                        Context Data
                        <span className="text-[9px] bg-dark-gray/50 px-1.5 py-0.5 rounded font-normal normal-case tracking-normal">
                          {Object.keys(selectedLog.context).length} fields
                        </span>
                        <button
                          type="button"
                          onClick={() =>
                            copyToClipboard(JSON.stringify(selectedLog.context, null, 2), 'context')
                          }
                          className="p-0.5 rounded hover:bg-dark-gray/50 transition-colors ml-auto"
                        >
                          {copied === 'context' ? (
                            <Check className="w-3 h-3 text-success" />
                          ) : (
                            <Copy className="w-3 h-3" />
                          )}
                        </button>
                      </div>
                      <div className="bg-elevated rounded-[var(--radius-md)] px-3 py-2 overflow-x-auto max-h-64 overflow-y-auto">
                        <JsonViewer data={selectedLog.context} collapsed={false} maxDepth={4} />
                      </div>
                    </div>
                  )}
                </div>
              </div>
            )}
          </div>

          {sourceCounts.length > 0 && (
            <div className="flex items-center gap-3 px-4 py-2 border-t border-border bg-dark-gray/20 text-[11px]">
              <span className="text-muted">Top sources:</span>
              {sourceCounts.map(([source, count]) => (
                <button
                  key={source}
                  type="button"
                  onClick={() => dispatchFilter({ type: 'SET_SEARCH_QUERY', payload: source })}
                  className="flex items-center gap-1.5 px-2 py-1 rounded bg-dark-gray/50 hover:bg-dark-gray transition-colors"
                >
                  <span className="text-foreground font-medium truncate max-w-[120px]">
                    {source}
                  </span>
                  <span className="text-muted">({count})</span>
                </button>
              ))}
            </div>
          )}
        </>
      )}

      {/* Fullscreen Log Modal */}
      {fullscreenLog && (
        // biome-ignore lint/a11y/noStaticElementInteractions: modal backdrop with keyboard support
        <div
          role="presentation"
          className="fixed inset-0 bg-background/95 backdrop-blur-sm flex items-center justify-center z-50 p-8"
          onClick={() => dispatchUi({ type: 'SET_FULLSCREEN_LOG_ID', payload: null })}
          onKeyDown={(e) => {
            if (e.key === 'Escape') dispatchUi({ type: 'SET_FULLSCREEN_LOG_ID', payload: null })
          }}
        >
          {/* biome-ignore lint/a11y/noStaticElementInteractions: stop propagation for modal content */}
          <div
            role="presentation"
            className="bg-background border border-border rounded-lg shadow-xl w-full max-w-5xl max-h-full flex flex-col overflow-hidden"
            onClick={(e) => e.stopPropagation()}
            onKeyDown={(e) => e.stopPropagation()}
          >
            {/* Header */}
            <div className="flex items-center justify-between px-6 py-4 border-b border-border bg-dark-gray/30">
              <div className="flex items-center gap-4">
                <span
                  className={`px-3 py-1 rounded text-xs font-bold uppercase ${LEVEL_CONFIG[fullscreenLog.level]?.badge}`}
                >
                  {fullscreenLog.level}
                </span>
                <div className="flex items-center gap-2 text-sm text-muted font-mono">
                  <Clock className="w-4 h-4" />
                  {formatFullTimestamp(fullscreenLog.time)}
                </div>
              </div>
              <div className="flex items-center gap-2">
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() =>
                    copyToClipboard(
                      JSON.stringify(
                        {
                          timestamp: fullscreenLog.timestamp,
                          level: fullscreenLog.level,
                          source: fullscreenLog.source,
                          message: fullscreenLog.message,
                          traceId: fullscreenLog.traceId,
                          context: fullscreenLog.context,
                        },
                        null,
                        2,
                      ),
                      'fullscreen-log',
                    )
                  }
                  className="h-8 text-xs gap-1.5"
                >
                  {copied === 'fullscreen-log' ? (
                    <Check className="w-3 h-3 text-success" />
                  ) : (
                    <Copy className="w-3 h-3" />
                  )}
                  Copy JSON
                </Button>
                <button
                  type="button"
                  onClick={() => dispatchUi({ type: 'SET_FULLSCREEN_LOG_ID', payload: null })}
                  className="p-2 rounded hover:bg-dark-gray transition-colors"
                >
                  <X className="w-5 h-5" />
                </button>
              </div>
            </div>

            {/* Content */}
            <div className="flex-1 overflow-auto p-6">
              <div className="space-y-6">
                {/* Message */}
                <div>
                  <div className="font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted mb-3 flex items-center gap-2">
                    <Terminal className="w-4 h-4" />
                    Message
                  </div>
                  <div
                    className={`text-lg font-medium leading-relaxed p-4 rounded-[var(--radius-lg)] bg-elevated border border-border-subtle ${LEVEL_CONFIG[fullscreenLog.level]?.text}`}
                  >
                    {fullscreenLog.message}
                  </div>
                </div>

                {/* Metadata Grid */}
                <div className="grid grid-cols-2 gap-4">
                  {/* Source */}
                  <div className="p-4 rounded-[var(--radius-lg)] bg-elevated border border-border-subtle">
                    <div className="font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted mb-2 flex items-center gap-2">
                      <Zap className="w-3.5 h-3.5" />
                      Source Function
                    </div>
                    <code className="font-mono text-[13px] text-cyan-400">
                      {fullscreenLog.source}
                    </code>
                  </div>

                  {/* Trace ID */}
                  {fullscreenLog.traceId && (
                    <div className="p-4 rounded-[var(--radius-lg)] bg-elevated border border-border-subtle">
                      <div className="font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted mb-2 flex items-center gap-2">
                        <ExternalLink className="w-3.5 h-3.5" />
                        Trace ID
                      </div>
                      <div className="flex items-center gap-2">
                        <code className="font-mono text-[13px] break-all">
                          {fullscreenLog.traceId}
                        </code>
                        <button
                          type="button"
                          onClick={() => {
                            filterByTrace(fullscreenLog.traceId ?? '')
                            dispatchUi({ type: 'SET_FULLSCREEN_LOG_ID', payload: null })
                          }}
                          className="p-1.5 rounded bg-primary/20 hover:bg-primary/30 text-primary transition-colors shrink-0"
                          title="Filter by this trace"
                        >
                          <Filter className="w-4 h-4" />
                        </button>
                      </div>
                    </div>
                  )}
                </div>

                {/* Timestamps */}
                <div className="p-4 rounded-[var(--radius-lg)] bg-elevated border border-border-subtle">
                  <div className="font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted mb-3 flex items-center gap-2">
                    <Clock className="w-3.5 h-3.5" />
                    Timestamps
                  </div>
                  <div className="grid grid-cols-3 gap-4">
                    <div>
                      <div className="font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted mb-1">
                        ISO 8601
                      </div>
                      <code className="font-mono text-[13px]">{fullscreenLog.timestamp}</code>
                    </div>
                    <div>
                      <div className="font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted mb-1">
                        Unix (ms)
                      </div>
                      <code className="font-mono text-[13px]">{fullscreenLog.time}</code>
                    </div>
                    <div>
                      <div className="font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted mb-1">
                        Local
                      </div>
                      <code className="font-mono text-[13px]">
                        {new Date(fullscreenLog.time).toLocaleString()}
                      </code>
                    </div>
                  </div>
                </div>

                {/* Context Data */}
                {fullscreenLog.context && Object.keys(fullscreenLog.context).length > 0 && (
                  <div>
                    <div className="font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted mb-3 flex items-center gap-2">
                      <CornerDownRight className="w-3.5 h-3.5" />
                      Context Data
                      <span className="text-[10px] bg-dark-gray px-2 py-0.5 rounded font-normal normal-case tracking-normal">
                        {Object.keys(fullscreenLog.context).length} fields
                      </span>
                      <button
                        type="button"
                        onClick={() =>
                          copyToClipboard(
                            JSON.stringify(fullscreenLog.context, null, 2),
                            'fullscreen-context',
                          )
                        }
                        className="p-1 rounded hover:bg-dark-gray/50 transition-colors ml-auto"
                      >
                        {copied === 'fullscreen-context' ? (
                          <Check className="w-3.5 h-3.5 text-success" />
                        ) : (
                          <Copy className="w-3.5 h-3.5" />
                        )}
                      </button>
                    </div>
                    <div className="bg-elevated p-4 rounded-[var(--radius-lg)] overflow-x-auto max-h-[400px] overflow-y-auto border border-border-subtle">
                      <JsonViewer data={fullscreenLog.context} collapsed={false} maxDepth={6} />
                    </div>
                  </div>
                )}
              </div>
            </div>

            {/* Footer */}
            <div className="px-6 py-3 border-t border-border text-xs text-muted flex items-center justify-between">
              <span>Press Escape or click outside to close</span>
              <span>Log ID: {fullscreenLog.id}</span>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
