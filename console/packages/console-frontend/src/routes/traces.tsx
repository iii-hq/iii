import { createFileRoute } from '@tanstack/react-router'
import {
  Activity,
  AlertCircle,
  CheckCircle2,
  Eye,
  EyeOff,
  GitBranch,
  Pause,
  Play,
  RefreshCw,
  Timer,
  XCircle,
  Zap,
} from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { fetchTraceTree } from '@/api'
import { FlameGraph } from '@/components/traces/FlameGraph'
import { FlowView } from '@/components/traces/FlowView'
import { ServiceBreakdown } from '@/components/traces/ServiceBreakdown'
import { SpanPanel } from '@/components/traces/SpanPanel'
import { TraceFilters } from '@/components/traces/TraceFilters'
import { TraceHeader } from '@/components/traces/TraceHeader'
import { TraceMap } from '@/components/traces/TraceMap'
import { ViewSwitcher, type ViewType } from '@/components/traces/ViewSwitcher'
import { WaterfallChart } from '@/components/traces/WaterfallChart'
import { Badge, Button } from '@/components/ui/card'
import { EmptyState } from '@/components/ui/empty-state'
import { ErrorBoundary } from '@/components/ui/error-boundary'
import { Pagination } from '@/components/ui/pagination'
import { Skeleton } from '@/components/ui/skeleton'
import { useResizablePanels } from '@/hooks/useResizablePanels'
import { type TraceGroup, useTraceData } from '@/hooks/useTraceData'
import { useTraceFilters } from '@/hooks/useTraceFilters'
import {
  treeToWaterfallData,
  type VisualizationSpan,
  type WaterfallData,
} from '@/lib/traceTransform'
import { formatDuration } from '@/lib/traceUtils'

export const Route = createFileRoute('/traces')({
  component: TracesPage,
})

function formatTime(timestamp: number): string {
  // If timestamp is in nanoseconds (beyond year 2100 in ms), convert to ms
  const timestampMs = timestamp > 4102444800000 ? timestamp / 1_000_000 : timestamp
  const date = new Date(timestampMs)

  if (Number.isNaN(date.getTime())) {
    return 'Invalid Date'
  }

  return date.toLocaleTimeString('en-US', {
    hour12: false,
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  })
}

function StatusIcon({ status }: { status: TraceGroup['status'] }) {
  switch (status) {
    case 'ok':
      return <CheckCircle2 className="w-3.5 h-3.5 text-success" />
    case 'error':
      return <XCircle className="w-3.5 h-3.5 text-error" />
    default:
      return <Activity className="w-3.5 h-3.5 text-yellow animate-pulse" />
  }
}

function TracesPage() {
  const [searchQuery, setSearchQuery] = useState('')
  const [debouncedSearch, setDebouncedSearch] = useState('')
  const searchTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const handleSearchChange = useCallback((value: string) => {
    setSearchQuery(value)
    if (searchTimerRef.current) clearTimeout(searchTimerRef.current)
    searchTimerRef.current = setTimeout(() => setDebouncedSearch(value), 300)
  }, [])
  const [showSystem, setShowSystem] = useState(false)
  const [selectedTraceId, setSelectedTraceId] = useState<string | null>(null)

  const [activeView, setActiveView] = useState<ViewType>('waterfall')
  const [selectedSpan, setSelectedSpan] = useState<VisualizationSpan | null>(null)
  const [waterfallData, setWaterfallData] = useState<WaterfallData | null>(null)
  const [isLoadingSpans, setIsLoadingSpans] = useState(false)
  const [spansError, setSpansError] = useState<string | null>(null)
  const [isPaused, setIsPaused] = useState(false)

  const {
    filters: filterState,
    updateFilter,
    resetFilters,
    getActiveFilterCount,
    getFilterOnlyParams,
    validationWarnings,
    clearValidationWarnings,
  } = useTraceFilters()

  const activeFilterCount = getActiveFilterCount()

  const filterParams = getFilterOnlyParams()

  const {
    traceGroups,
    newTraceIds,
    setNewTraceIds,
    hasOtelConfigured,
    isQueryLoading,
    refetch,
    isHoveredRef,
    flushPendingTraces,
  } = useTraceData({
    filterParams,
    showSystem,
    debouncedSearch,
    isPaused,
  })

  const loadTraceSpans = useCallback(async (traceId: string) => {
    setIsLoadingSpans(true)
    setSpansError(null)
    setWaterfallData(null)

    try {
      const data = await fetchTraceTree(traceId)

      if (data.roots && data.roots.length > 0) {
        const wfData = treeToWaterfallData(data.roots)

        if (wfData) {
          setWaterfallData(wfData)
        } else {
          console.warn('[Traces] treeToWaterfallData returned null')
          setSpansError('Failed to process span data')
        }
      } else {
        console.warn('[Traces] No roots found for trace:', traceId)
        setSpansError('No span data available for this trace')
      }
    } catch (error) {
      console.error('[Traces] Failed to load trace tree:', error)
      setSpansError(error instanceof Error ? error.message : 'Failed to load trace details')
    } finally {
      setIsLoadingSpans(false)
    }
  }, [])

  const selectTrace = useCallback(
    (traceId: string | null) => {
      setSelectedTraceId(traceId)
      if (!traceId) {
        setWaterfallData(null)
        setSelectedSpan(null)
        setSpansError(null)
        setIsLoadingSpans(false)
      } else {
        setIsPaused(true)
        loadTraceSpans(traceId)
      }
    },
    [loadTraceSpans],
  )

  const selectedTrace = traceGroups.find((g) => g.traceId === selectedTraceId)

  const filteredTraces = useMemo(() => {
    // When debouncedSearch is active, server already filtered by name across all spans
    // Only apply client-side filter for the interim before debounce fires
    const pendingClientSearch = searchQuery && searchQuery !== debouncedSearch
    return traceGroups.filter((group) => {
      if (pendingClientSearch) {
        const query = searchQuery.toLowerCase()
        const matchesId = group.traceId.toLowerCase().includes(query)
        const matchesOp = group.rootOperation.toLowerCase().includes(query)
        const matchesFn = group.functionId?.toLowerCase().includes(query) ?? false
        const matchesTopic = group.topic?.toLowerCase().includes(query) ?? false
        if (!matchesId && !matchesOp && !matchesFn && !matchesTopic) return false
      }
      return true
    })
  }, [traceGroups, searchQuery, debouncedSearch])

  const totalPages = Math.max(1, Math.ceil(filteredTraces.length / filterState.pageSize))

  const pagedTraces = useMemo(() => {
    const start = (filterState.page - 1) * filterState.pageSize
    return filteredTraces.slice(start, start + filterState.pageSize)
  }, [filteredTraces, filterState.page, filterState.pageSize])

  useEffect(() => {
    if (selectedTraceId && !pagedTraces.some((g) => g.traceId === selectedTraceId)) {
      selectTrace(null)
    }
  }, [pagedTraces, selectedTraceId, selectTrace])

  const stats = useMemo(
    () => ({
      totalTraces: filteredTraces.length,
      errorCount: filteredTraces.filter((g) => g.status === 'error').length,
      avgDuration:
        filteredTraces.length > 0
          ? filteredTraces.reduce((sum, g) => sum + (g.duration || 0), 0) / filteredTraces.length
          : 0,
    }),
    [filteredTraces],
  )

  // --- Resizable panels ---
  const containerRef = useRef<HTMLDivElement>(null)

  const { panelWidths, isResizing, startResize, resetTracePanel, resetSpanPanel } =
    useResizablePanels({
      selectedSpanId: selectedSpan?.span_id ?? null,
      containerRef,
    })

  return (
    <div className="flex flex-col h-full bg-background text-foreground">
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 px-4 md:px-6 py-3 md:py-4 bg-dark-gray/30 border-b border-border">
        <div className="flex items-center gap-2 md:gap-4 flex-wrap">
          <h1 className="font-sans font-semibold text-lg tracking-tight flex items-center gap-2">
            <GitBranch className="w-5 h-5 text-accent" />
            Traces
          </h1>
          {isPaused && (
            <Badge variant="warning" className="gap-1 text-[10px] md:text-xs">
              <Pause className="w-2.5 h-2.5 md:w-3 md:h-3" />
              <span className="hidden sm:inline">Paused</span>
            </Badge>
          )}
        </div>

        <div className="flex items-center gap-1.5 md:gap-2">
          <Button
            variant={showSystem ? 'accent' : 'ghost'}
            size="sm"
            onClick={() => setShowSystem(!showSystem)}
            className="h-6 md:h-7 text-[10px] md:text-xs px-2"
          >
            {showSystem ? (
              <Eye className="w-3 h-3 md:mr-1.5" />
            ) : (
              <EyeOff className="w-3 h-3 md:mr-1.5" />
            )}
            <span className={`hidden md:inline ${showSystem ? '' : 'line-through opacity-60'}`}>
              System
            </span>
          </Button>
          <Button
            variant={isPaused ? 'accent' : 'ghost'}
            size="sm"
            onClick={() => setIsPaused(!isPaused)}
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
            onClick={() => refetch()}
            disabled={isQueryLoading}
            className="h-7 text-xs text-muted hover:text-foreground"
          >
            <RefreshCw className={`w-3.5 h-3.5 mr-1.5 ${isQueryLoading ? 'animate-spin' : ''}`} />
            Refresh
          </Button>
        </div>
      </div>

      <div className="px-4 py-2 border-b border-border">
        <ErrorBoundary>
          <TraceFilters
            filters={filterState}
            onFilterChange={updateFilter}
            onClear={resetFilters}
            validationWarnings={validationWarnings}
            onClearWarnings={clearValidationWarnings}
            isLoading={isQueryLoading}
            searchQuery={searchQuery}
            onSearchChange={handleSearchChange}
            stats={hasOtelConfigured ? stats : undefined}
          />
        </ErrorBoundary>
      </div>

      <div className="flex-1 flex overflow-hidden" ref={containerRef}>
        <div
          className={`flex flex-col flex-1 overflow-hidden ${selectedSpan && waterfallData ? 'hidden' : ''}`}
        >
          {/* biome-ignore lint/a11y/noStaticElementInteractions: hover detection for pause/resume of live updates */}
          <div
            className="flex-1 overflow-y-auto"
            onMouseEnter={() => {
              isHoveredRef.current = true
            }}
            onMouseLeave={() => {
              isHoveredRef.current = false
              flushPendingTraces()
            }}
          >
            {isQueryLoading && traceGroups.length === 0 ? (
              <div className="flex flex-col gap-0">
                {(['tr-sk-0', 'tr-sk-1', 'tr-sk-2', 'tr-sk-3', 'tr-sk-4'] as const).map((sk) => (
                  <div key={sk} className="p-3 border-b border-border">
                    <div className="flex items-center gap-2 mb-2">
                      <Skeleton className="w-3.5 h-3.5 rounded-full" />
                      <Skeleton className="h-4 w-48" />
                    </div>
                    <div className="flex items-center gap-3">
                      <Skeleton className="h-3 w-16" />
                      <Skeleton className="h-3 w-12" />
                      <Skeleton className="h-3 w-20" />
                      <Skeleton className="h-3 w-14 ml-auto" />
                    </div>
                  </div>
                ))}
              </div>
            ) : filteredTraces.length === 0 ? (
              <div className="flex-1 flex items-center justify-center p-12">
                {activeFilterCount > 0 || searchQuery ? (
                  <EmptyState
                    icon={GitBranch}
                    title="No traces found"
                    description="No traces match the current filters. Try adjusting or clearing them."
                    action={{
                      label: 'Clear all filters',
                      onClick: () => {
                        resetFilters()
                        handleSearchChange('')
                      },
                    }}
                  />
                ) : (
                  <EmptyState
                    icon={GitBranch}
                    title="No traces recorded"
                    description="Traces are captured when functions execute"
                  />
                )}
              </div>
            ) : (
              pagedTraces.map((group) => {
                const isSelected = selectedTraceId === group.traceId
                const isNew = newTraceIds.has(group.traceId)

                return (
                  <button
                    key={group.traceId}
                    type="button"
                    onClick={() => selectTrace(isSelected ? null : group.traceId)}
                    onAnimationEnd={() => {
                      if (isNew)
                        setNewTraceIds((prev) => {
                          const next = new Set(prev)
                          next.delete(group.traceId)
                          return next
                        })
                    }}
                    className={`w-full p-3 border-b border-border text-left transition-colors
                      ${isSelected ? 'bg-primary/10 border-l-2 border-l-primary' : 'hover:bg-dark-gray/50'}
                      ${isNew ? 'animate-trace-flash' : ''}
                    `}
                  >
                    <div className="flex items-center gap-2 mb-1">
                      <StatusIcon status={group.status} />
                      <span className="font-sans font-medium text-sm truncate flex-1">
                        {group.topic ? (
                          <>
                            <span className="font-sans text-muted text-xs font-normal mr-1">
                              enqueue:
                            </span>
                            {group.topic}
                          </>
                        ) : (
                          (group.functionId ?? group.rootOperation)
                        )}
                      </span>
                    </div>

                    <div className="flex items-center gap-3 text-[10px] text-muted">
                      <code className="font-mono text-[13px]">{group.traceId.slice(0, 8)}</code>
                      <span className="flex items-center gap-1">
                        <Timer className="w-2.5 h-2.5" />
                        {formatDuration(group.duration ?? 0)}
                      </span>
                      <span className="flex items-center gap-1">
                        <Zap className="w-2.5 h-2.5" />
                        {group.services.join(', ')}
                      </span>
                      <span className="ml-auto">{formatTime(group.startTime)}</span>
                    </div>
                  </button>
                )
              })
            )}
          </div>

          {filteredTraces.length > 0 && (
            <div className="flex-shrink-0 bg-background/95 backdrop-blur border-t border-border px-3 py-2">
              <Pagination
                currentPage={filterState.page}
                totalPages={totalPages}
                totalItems={filteredTraces.length}
                pageSize={filterState.pageSize}
                onPageChange={(page) => updateFilter('page', page)}
                onPageSizeChange={(pageSize) => updateFilter('pageSize', pageSize)}
                pageSizeOptions={[25, 50, 100]}
              />
            </div>
          )}
        </div>

        {selectedTrace && (
          <>
            {!(selectedSpan && waterfallData) && (
              <button
                type="button"
                onMouseDown={(e) => startResize(e, 'trace')}
                onDoubleClick={resetTracePanel}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' || e.key === ' ') {
                    e.preventDefault()
                    resetTracePanel()
                  }
                }}
                className="w-[3px] flex-shrink-0 cursor-col-resize relative bg-border hover:bg-primary/50 active:bg-primary transition-colors"
              >
                <div className="absolute inset-y-0 -left-[3px] -right-[3px]" />
              </button>
            )}
            <div
              style={{ width: panelWidths.trace }}
              className={`bg-sidebar flex flex-col h-full overflow-hidden flex-shrink-0 animate-trace-panel-in ${isResizing ? 'pointer-events-none select-none' : ''}`}
            >
              {isLoadingSpans && (
                <div className="flex-1 flex flex-col p-4 gap-3">
                  <div className="flex items-center gap-2">
                    <Skeleton className="w-4 h-4 rounded-full" />
                    <Skeleton className="h-4 w-32" />
                    <Skeleton className="h-4 w-20 ml-auto" />
                  </div>
                  {(['tr-p-sk-0', 'tr-p-sk-1', 'tr-p-sk-2', 'tr-p-sk-3', 'tr-p-sk-4'] as const).map(
                    (sk) => (
                      <div key={sk} className="flex items-center gap-2">
                        <Skeleton className="h-6 w-full" />
                      </div>
                    ),
                  )}
                </div>
              )}

              {!isLoadingSpans && spansError && (
                <div className="flex-1 flex flex-col items-center justify-center p-8">
                  <div className="w-10 h-10 mb-3 rounded-lg bg-dark-gray border border-border flex items-center justify-center">
                    <AlertCircle className="w-5 h-5 text-error" />
                  </div>
                  <div className="text-xs font-medium mb-1 text-error">Failed to load trace</div>
                  <div className="text-[10px] text-muted text-center mb-3 max-w-xs">
                    {spansError}
                  </div>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => loadTraceSpans(selectedTrace.traceId)}
                    className="text-[10px] h-6"
                  >
                    <RefreshCw className="w-3 h-3 mr-1" />
                    Retry
                  </Button>
                </div>
              )}

              {!isLoadingSpans && !spansError && waterfallData && (
                <>
                  <TraceHeader
                    data={waterfallData}
                    traceId={selectedTrace.traceId}
                    onClose={() => selectTrace(null)}
                    onSpanClick={setSelectedSpan}
                  />

                  <div className="border-b border-border-subtle px-4 py-2.5">
                    <ViewSwitcher currentView={activeView} onViewChange={setActiveView} />
                  </div>

                  <div className="flex-1 overflow-auto min-h-0">
                    {activeView === 'waterfall' && (
                      <WaterfallChart
                        data={waterfallData}
                        onSpanClick={setSelectedSpan}
                        selectedSpanId={selectedSpan?.span_id}
                      />
                    )}

                    {activeView === 'flamegraph' && (
                      <FlameGraph
                        data={waterfallData}
                        onSpanClick={setSelectedSpan}
                        selectedSpanId={selectedSpan?.span_id}
                      />
                    )}

                    {activeView === 'map' && (
                      <TraceMap data={waterfallData} onSpanClick={setSelectedSpan} />
                    )}

                    {activeView === 'flow' && (
                      <FlowView
                        data={waterfallData}
                        onSpanClick={setSelectedSpan}
                        selectedSpanId={selectedSpan?.span_id}
                      />
                    )}
                  </div>

                  {activeView !== 'flow' && (
                    <div className="border-t border-border-subtle flex-shrink-0">
                      <ServiceBreakdown data={waterfallData} />
                    </div>
                  )}
                </>
              )}
            </div>
          </>
        )}

        {selectedSpan && waterfallData && (
          <>
            <button
              type="button"
              onMouseDown={(e) => startResize(e, 'span')}
              onDoubleClick={resetSpanPanel}
              onKeyDown={(e) => {
                if (e.key === 'Enter' || e.key === ' ') {
                  e.preventDefault()
                  resetSpanPanel()
                }
              }}
              className="w-[3px] flex-shrink-0 cursor-col-resize relative bg-border hover:bg-primary/50 active:bg-primary transition-colors"
            >
              <div className="absolute inset-y-0 -left-[3px] -right-[3px]" />
            </button>
            <div
              style={{ width: panelWidths.span }}
              className={`bg-sidebar flex-shrink-0 h-full overflow-hidden ${isResizing ? 'pointer-events-none select-none' : ''}`}
            >
              <SpanPanel
                key={selectedSpan.span_id}
                span={selectedSpan}
                traceData={waterfallData}
                onClose={() => setSelectedSpan(null)}
                onNavigateToSpan={setSelectedSpan}
                onNavigateToTrace={selectTrace}
              />
            </div>
          </>
        )}
      </div>
    </div>
  )
}
