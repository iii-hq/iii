import { useQuery } from '@tanstack/react-query'
import { ArrowUp, Clock, Copy, Layers, X, Zap } from 'lucide-react'
import { useEffect, useMemo } from 'react'
import { fetchOtelLogs } from '@/api'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import type { VisualizationSpan, WaterfallData } from '@/lib/traceTransform'
import { formatDuration, getServiceName, STATUS_CONFIG, useCopyToClipboard } from '@/lib/traceUtils'
import { SpanBaggageTab } from './SpanBaggageTab'
import { SpanErrorsTab } from './SpanErrorsTab'
import { SpanInfoTab } from './SpanInfoTab'
import { SpanLinksTab } from './SpanLinksTab'
import { SpanLogsTab } from './SpanLogsTab'
import { SpanOtelLogsTab } from './SpanOtelLogsTab'
import { SpanTagsTab } from './SpanTagsTab'

interface SpanPanelProps {
  span: VisualizationSpan | null
  traceData: WaterfallData | null
  onClose: () => void
  onNavigateToSpan: (span: VisualizationSpan) => void
  onNavigateToTrace?: (traceId: string) => void
}

export function SpanPanel({
  span,
  traceData,
  onClose,
  onNavigateToSpan,
  onNavigateToTrace,
}: SpanPanelProps) {
  const { copiedKey: copiedField, copy: copyToClipboard } = useCopyToClipboard()

  useEffect(() => {
    const handleEscape = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    document.addEventListener('keydown', handleEscape)
    return () => document.removeEventListener('keydown', handleEscape)
  }, [onClose])

  const traceContext = useMemo(() => {
    if (!span || !traceData) return null
    const parentSpan = traceData.spans.find((s) => s.span_id === span.parent_span_id)
    const childSpans = traceData.spans.filter((s) => s.parent_span_id === span.span_id)
    const childDuration = childSpans.reduce((sum, child) => sum + child.duration_ms, 0)
    // Note: self-time is approximate when child spans overlap (parallel execution)
    const selfTime = Math.max(0, span.duration_ms - childDuration)
    return { parentSpan, childSpans, selfTime, childDuration }
  }, [span, traceData])

  const { data: logsData } = useQuery({
    queryKey: ['span-otel-logs', span?.trace_id, span?.span_id],
    queryFn: () => fetchOtelLogs({ trace_id: span?.trace_id ?? undefined, span_id: span?.span_id }),
    enabled: !!span?.trace_id && !!span?.span_id,
  })
  const logCount = logsData?.logs?.length ?? 0

  if (!span) return null

  const hasError = span.status === 'error'
  const attrCount = Object.keys(span.attributes || {}).length
  const eventCount = span.events?.length || 0
  const linkCount = span.links?.length || 0
  const service = getServiceName(span)
  const statusConfig = STATUS_CONFIG[span.status] ?? STATUS_CONFIG.default

  return (
    <div className="h-full bg-[#0A0A0A] overflow-hidden flex flex-col animate-panel-in">
      {/* Compact header */}
      <div className="flex-shrink-0 border-b border-[#1D1D1D]">
        {/* Row 1: service badge + span name + close */}
        <div className="flex items-center gap-2 px-4 pt-3 pb-1.5">
          <div
            className="px-1.5 py-0.5 rounded text-[10px] font-medium tracking-wide flex-shrink-0"
            style={{
              backgroundColor: statusConfig.bg,
              color: statusConfig.color,
              border: `1px solid ${statusConfig.border}`,
            }}
          >
            {service}
          </div>
          <h2
            className="text-sm font-semibold text-[#F4F4F4] leading-tight truncate flex-1 min-w-0"
            title={span.name}
          >
            {span.name}
          </h2>
          <button
            type="button"
            onClick={onClose}
            className="p-1 hover:bg-[#1D1D1D] rounded-md transition-colors group flex-shrink-0"
            aria-label="Close panel (Esc)"
            title="Close (Esc)"
          >
            <X className="w-3.5 h-3.5 text-gray-500 group-hover:text-gray-300" />
          </button>
        </div>

        {/* Row 2: span ID + quick stats */}
        <div className="flex items-center gap-2 px-4 pb-2.5 flex-wrap">
          <button
            type="button"
            onClick={() => copyToClipboard('spanId', span.span_id)}
            className="flex items-center gap-1 text-[10px] text-gray-500 hover:text-gray-300 transition-colors font-mono group"
          >
            <span>{span.span_id.slice(0, 12)}</span>
            {copiedField === 'spanId' ? (
              <span className="text-[#22C55E] text-[9px]">copied</span>
            ) : (
              <Copy className="w-2.5 h-2.5 opacity-0 group-hover:opacity-100 transition-opacity" />
            )}
          </button>

          <div className="w-px h-3 bg-[#1D1D1D]" />

          <div className="flex items-center gap-1 px-2 py-0.5 bg-[#141414] border border-[#1D1D1D] rounded">
            <Clock className="w-2.5 h-2.5 text-[#F3F724]" />
            <span className="text-[11px] font-mono font-semibold text-[#F3F724]">
              {formatDuration(span.duration_ms)}
            </span>
          </div>

          {traceContext && traceContext.childSpans.length > 0 && (
            <div className="flex items-center gap-1 px-2 py-0.5 bg-[#141414] border border-[#1D1D1D] rounded">
              <Zap className="w-2.5 h-2.5 text-gray-400" />
              <span className="text-[10px] font-mono text-gray-400">
                self {formatDuration(traceContext.selfTime)}
              </span>
            </div>
          )}

          <div
            className="flex items-center gap-1 px-2 py-0.5 rounded"
            style={{
              backgroundColor: statusConfig.bg,
              border: `1px solid ${statusConfig.border}`,
            }}
          >
            <div
              className="w-1.5 h-1.5 rounded-full"
              style={{ backgroundColor: statusConfig.color }}
            />
            <span
              className="text-[10px] font-mono font-medium"
              style={{ color: statusConfig.color }}
            >
              {statusConfig.label}
            </span>
          </div>

          <div className="flex items-center gap-1 px-2 py-0.5 bg-[#141414] border border-[#1D1D1D] rounded">
            <Layers className="w-2.5 h-2.5 text-gray-400" />
            <span className="text-[10px] font-mono text-gray-400">d:{span.depth}</span>
          </div>

          {traceContext && traceContext.childSpans.length > 0 && (
            <div className="flex items-center gap-1 px-2 py-0.5 bg-[#141414] border border-[#1D1D1D] rounded">
              <span className="text-[10px] font-mono text-gray-400">
                {traceContext.childSpans.length} child
                {traceContext.childSpans.length !== 1 ? 'ren' : ''}
              </span>
            </div>
          )}
        </div>

        {/* Parent span navigation (compact) */}
        {traceContext?.parentSpan && (
          <div className="px-4 pb-2.5">
            <button
              type="button"
              onClick={() => traceContext.parentSpan && onNavigateToSpan(traceContext.parentSpan)}
              className="flex items-center gap-1.5 w-full px-2.5 py-1.5 bg-[#141414] border border-[#1D1D1D] rounded hover:bg-[#1A1A1A] hover:border-[#252525] transition-colors group text-left"
            >
              <ArrowUp className="w-3 h-3 text-gray-500 group-hover:text-[#F3F724] transition-colors flex-shrink-0" />
              <span className="text-[10px] text-gray-500 flex-shrink-0">parent</span>
              <span className="text-[10px] text-gray-300 truncate group-hover:text-[#F4F4F4] transition-colors">
                {traceContext.parentSpan.name}
              </span>
              <span className="text-[9px] font-mono text-gray-600 ml-auto flex-shrink-0">
                {formatDuration(traceContext.parentSpan.duration_ms)}
              </span>
            </button>
          </div>
        )}
      </div>

      {/* Tabbed content */}
      <div className="flex-1 overflow-hidden min-h-0">
        <Tabs defaultValue={hasError ? 'errors' : 'info'} className="h-full flex flex-col">
          <TabsList className="px-4 pt-2 pb-0 bg-transparent">
            <TabsTrigger value="info" className="text-[11px]">
              Info
            </TabsTrigger>
            <TabsTrigger value="tags" className="text-[11px]">
              Attributes
              {attrCount > 0 && (
                <span className="ml-1 px-1 py-0.5 text-[9px] bg-[#1D1D1D] rounded font-mono">
                  {attrCount}
                </span>
              )}
            </TabsTrigger>
            <TabsTrigger value="logs" className="text-[11px]">
              Events
              {eventCount > 0 && (
                <span className="ml-1 px-1 py-0.5 text-[9px] bg-[#1D1D1D] rounded font-mono">
                  {eventCount}
                </span>
              )}
            </TabsTrigger>
            <TabsTrigger value="errors" className="text-[11px]">
              Errors
              {hasError && (
                <span className="ml-1 w-1.5 h-1.5 bg-red-500 rounded-full inline-block" />
              )}
            </TabsTrigger>
            <TabsTrigger value="otel-logs" className="text-[11px]">
              Logs
              {logCount > 0 && (
                <span className="ml-1 px-1 py-0.5 text-[9px] bg-[#1D1D1D] rounded font-mono">
                  {logCount}
                </span>
              )}
            </TabsTrigger>
            <TabsTrigger value="baggage" className="text-[11px]">
              Context
            </TabsTrigger>
            {linkCount > 0 && (
              <TabsTrigger value="links" className="text-[11px]">
                Links
                <span className="ml-1 px-1 py-0.5 text-[9px] bg-[#1D1D1D] rounded font-mono">
                  {linkCount}
                </span>
              </TabsTrigger>
            )}
          </TabsList>

          <div className="flex-1 overflow-y-auto min-h-0">
            <TabsContent value="info" className="mt-0">
              <SpanInfoTab span={span} traceData={traceData} />
            </TabsContent>
            <TabsContent value="tags" className="mt-0">
              <SpanTagsTab span={span} />
            </TabsContent>
            <TabsContent value="logs" className="mt-0">
              <SpanLogsTab span={span} />
            </TabsContent>
            <TabsContent value="errors" className="mt-0">
              <SpanErrorsTab span={span} />
            </TabsContent>
            <TabsContent value="otel-logs" className="mt-0">
              <SpanOtelLogsTab span={span} />
            </TabsContent>
            <TabsContent value="baggage" className="mt-0">
              <SpanBaggageTab span={span} />
            </TabsContent>
            {linkCount > 0 && (
              <TabsContent value="links" className="mt-0">
                <SpanLinksTab span={span} onNavigateToTrace={onNavigateToTrace} />
              </TabsContent>
            )}
          </div>
        </Tabs>
      </div>
    </div>
  )
}
