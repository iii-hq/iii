// Session detail panel: stacked vertical view of every trace inside a
// session-level group.
//
// When the user clicks a group row in the TRACES tab (with `Group by`
// active), `TraceGroupsView` previously surfaced only the FIRST trace in
// `group.trace_ids`. That caused a confusing UX: the row would say
// "26 spans" but the right-side detail panel would show 4 (just the
// first trace). This component fixes that by fetching every trace in
// the group in parallel and rendering each as a collapsible card with
// its own mini-waterfall.
//
// Each TraceCard reuses the existing render path
// (`treeToWaterfallData` + `WaterfallChart`) so spans behave identically
// to the single-trace view. Span clicks bubble up via `onSpanClick`,
// keeping the right-side span detail panel working unchanged.

import { useQueries } from '@tanstack/react-query'
import { ChevronDown, ChevronRight, Clock, Loader2, X } from 'lucide-react'
import { useMemo, useState } from 'react'

import { fetchTraceTree, type TraceGroup, type TraceTreeResponse } from '@/api/observability/traces'
import { useEngineSdk } from '@/api/engine-sdk-provider'
import { WaterfallChart } from '@/components/traces/WaterfallChart'
import {
  treeToWaterfallData,
  type VisualizationSpan,
  type WaterfallData,
} from '@/lib/traceTransform'

interface SessionDetailPanelProps {
  group: TraceGroup
  onClose: () => void
  onSpanClick: (span: VisualizationSpan) => void
  selectedSpanId?: string
}

interface TraceFetchResult {
  trace_id: string
  data?: WaterfallData
  isLoading: boolean
  error?: string
}

export function SessionDetailPanel({
  group,
  onClose,
  onSpanClick,
  selectedSpanId,
}: SessionDetailPanelProps) {
  // Fetch every trace in the group concurrently. React Query handles
  // dedup + caching automatically — if the user opens the same session
  // twice, the second open is instant.
  const sdk = useEngineSdk()
  const queries = useQueries({
    queries: group.trace_ids.map((traceId) => ({
      queryKey: ['trace-tree', traceId],
      queryFn: () => fetchTraceTree(sdk, traceId),
      staleTime: 30_000,
    })),
  })

  const traces: TraceFetchResult[] = useMemo(
    () =>
      group.trace_ids.map((traceId, idx): TraceFetchResult => {
        const q = queries[idx]
        if (q.isLoading) return { trace_id: traceId, isLoading: true }
        if (q.isError)
          return {
            trace_id: traceId,
            isLoading: false,
            error: q.error instanceof Error ? q.error.message : 'Failed to load',
          }
        const data = q.data ? buildWaterfall(q.data) : undefined
        return { trace_id: traceId, isLoading: false, data }
      }),
    [group.trace_ids, queries],
  )

  const totalSpans = group.span_count
  const traceCount = group.trace_ids.length
  const errorCount = group.error_count
  const durationSec = (group.duration_ms / 1000).toFixed(2)

  return (
    <div className="flex flex-col h-full overflow-hidden">
      {/* Header */}
      <div className="border-b border-border px-4 py-3 flex items-center gap-3 flex-shrink-0">
        <div className="flex-1 min-w-0">
          <div className="text-[11px] font-medium text-foreground truncate">
            session <span className="font-mono">{group.value}</span>
          </div>
          <div className="flex items-center gap-3 mt-0.5 text-[10px] text-muted">
            <span>
              {traceCount} trace{traceCount === 1 ? '' : 's'}
            </span>
            <span>•</span>
            <span>{totalSpans} spans</span>
            <span>•</span>
            <span className="flex items-center gap-1">
              <Clock className="w-3 h-3" />
              {durationSec}s
            </span>
            {errorCount > 0 && (
              <>
                <span>•</span>
                <span className="text-red-400">
                  {errorCount} error{errorCount === 1 ? '' : 's'}
                </span>
              </>
            )}
          </div>
        </div>
        <button
          type="button"
          onClick={onClose}
          aria-label="Close session detail"
          className="text-muted hover:text-foreground transition-colors p-1"
        >
          <X className="w-4 h-4" />
        </button>
      </div>

      {/* Stacked trace cards */}
      <div className="flex-1 overflow-y-auto min-h-0">
        {traces.map((trace, idx) => (
          <TraceCard
            key={trace.trace_id}
            index={idx + 1}
            trace={trace}
            onSpanClick={onSpanClick}
            selectedSpanId={selectedSpanId}
            // Auto-expand the first card so the user sees content
            // immediately. Subsequent cards stay collapsed to keep the
            // initial render light for sessions with many traces.
            defaultOpen={idx === 0}
          />
        ))}
      </div>
    </div>
  )
}

interface TraceCardProps {
  index: number
  trace: TraceFetchResult
  onSpanClick: (span: VisualizationSpan) => void
  selectedSpanId?: string
  defaultOpen: boolean
}

function TraceCard({ index, trace, onSpanClick, selectedSpanId, defaultOpen }: TraceCardProps) {
  const [open, setOpen] = useState(defaultOpen)

  const headerLabel = trace.data?.spans[0]?.name ?? 'loading…'
  const durationMs = trace.data?.total_duration_ms ?? 0
  const spanCount = trace.data?.span_count ?? 0
  const tracePreview = trace.trace_id.slice(0, 12)

  return (
    <div className="border-b border-border">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="w-full px-4 py-2.5 flex items-center gap-2 text-left hover:bg-dark-gray/50 transition-colors"
      >
        {open ? (
          <ChevronDown className="w-3.5 h-3.5 text-muted flex-shrink-0" />
        ) : (
          <ChevronRight className="w-3.5 h-3.5 text-muted flex-shrink-0" />
        )}
        <span className="text-[10px] font-mono text-muted w-6 flex-shrink-0">#{index}</span>
        <span className="text-[12px] truncate text-foreground flex-1">{headerLabel}</span>
        <span className="text-[10px] font-mono text-muted flex-shrink-0">{tracePreview}…</span>
        <span className="text-[10px] text-muted flex-shrink-0 w-12 text-right">
          {spanCount > 0 ? `${spanCount} sp` : ''}
        </span>
        <span className="text-[10px] text-muted flex-shrink-0 w-16 text-right">
          {durationMs > 0 ? `${durationMs.toFixed(0)}ms` : ''}
        </span>
      </button>

      {open && (
        <div className="bg-sidebar/40">
          {trace.isLoading && (
            <div className="flex items-center justify-center py-6 gap-2 text-[11px] text-muted">
              <Loader2 className="w-3.5 h-3.5 animate-spin" />
              Loading trace…
            </div>
          )}
          {trace.error && (
            <div className="px-4 py-3 text-[11px] text-red-400">{trace.error}</div>
          )}
          {trace.data && (
            <WaterfallChart
              data={trace.data}
              onSpanClick={onSpanClick}
              selectedSpanId={selectedSpanId}
            />
          )}
        </div>
      )}
    </div>
  )
}

function buildWaterfall(resp: TraceTreeResponse): WaterfallData | undefined {
  if (!resp.roots || resp.roots.length === 0) return undefined
  const wf = treeToWaterfallData(resp.roots)
  return wf ?? undefined
}
