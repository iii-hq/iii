import { AlertCircle, ChevronRight, Clock, Copy, Layers, X } from 'lucide-react'
import { Fragment, useMemo } from 'react'
import { getServiceColor } from '@/lib/traceColors'
import type { VisualizationSpan, WaterfallData } from '@/lib/traceTransform'
import { formatDuration, getServiceName, useCopyToClipboard } from '@/lib/traceUtils'

interface TraceHeaderProps {
  data: WaterfallData
  traceId: string
  onClose: () => void
  onSpanClick?: (span: VisualizationSpan) => void
}

export function TraceHeader({ data, traceId, onClose, onSpanClick }: TraceHeaderProps) {
  const { copiedKey, copy } = useCopyToClipboard()
  const copied = copiedKey === 'traceId'
  const rootSpan = data.spans.find((s) => s.depth === 0)

  const criticalPath = useMemo(() => {
    if (!data.spans.length) return []

    const childrenMap = new Map<string, VisualizationSpan[]>()
    const rootSpans: VisualizationSpan[] = []

    for (const span of data.spans) {
      if (!span.parent_span_id) {
        rootSpans.push(span)
      } else {
        const children = childrenMap.get(span.parent_span_id) || []
        children.push(span)
        childrenMap.set(span.parent_span_id, children)
      }
    }

    const path: VisualizationSpan[] = []
    let current = rootSpans.sort((a, b) => b.duration_ms - a.duration_ms)[0]

    while (current) {
      path.push(current)
      const children = childrenMap.get(current.span_id) || []
      current = children.sort((a, b) => b.duration_ms - a.duration_ms)[0]
    }

    return path
  }, [data.spans])

  const { errorCount, serviceList, serviceDurations } = useMemo(() => {
    let errorCount = 0
    const services = new Set<string>()
    const durationMap = new Map<string, number>()

    for (const span of data.spans) {
      if (span.status === 'error') errorCount++
      const svc = getServiceName(span)
      if (span.service_name) services.add(span.service_name)
      durationMap.set(svc, (durationMap.get(svc) || 0) + span.duration_ms)
    }

    const serviceList = Array.from(services).filter((s): s is string => Boolean(s))
    return { errorCount, serviceList, serviceDurations: durationMap }
  }, [data])

  const hasErrors = errorCount > 0
  const serviceCount = Math.max(serviceList.length, 1)
  const rootService = rootSpan ? getServiceName(rootSpan) : 'trace'

  return (
    <div className="bg-[#0A0A0A] border-b border-[#1D1D1D] flex-shrink-0">
      {/* Row 1: service badge + operation name + close */}
      <div className="flex items-center gap-2 px-4 pt-3 pb-1.5">
        <div className="px-1.5 py-0.5 rounded text-[10px] font-medium tracking-wide flex-shrink-0 bg-[#22C55E]/8 text-[#22C55E] border border-[#22C55E]/15">
          {rootService}
        </div>
        <h2
          className="text-sm font-semibold text-[#F4F4F4] leading-tight truncate flex-1 min-w-0"
          title={rootSpan?.name || 'Trace Details'}
        >
          {rootSpan?.name || 'Trace Details'}
        </h2>
        <button
          type="button"
          onClick={onClose}
          className="p-1 hover:bg-[#1D1D1D] rounded-md transition-colors group flex-shrink-0"
          aria-label="Close trace detail"
          title="Close (Esc)"
        >
          <X className="w-3.5 h-3.5 text-gray-500 group-hover:text-gray-300" />
        </button>
      </div>

      {/* Row 2: trace ID + stat pills */}
      <div className="flex items-center gap-2 px-4 pb-2.5 flex-wrap">
        <button
          type="button"
          onClick={() => copy('traceId', traceId)}
          className="flex items-center gap-1 text-[10px] text-gray-500 hover:text-gray-300 transition-colors font-mono group"
        >
          <span>{traceId.substring(0, 12)}</span>
          {copied ? (
            <span className="text-[#22C55E] text-[9px]">copied</span>
          ) : (
            <Copy className="w-2.5 h-2.5 opacity-0 group-hover:opacity-100 transition-opacity" />
          )}
        </button>

        <div className="w-px h-3 bg-[#1D1D1D]" />

        <div className="flex items-center gap-1 px-2 py-0.5 bg-[#141414] border border-[#1D1D1D] rounded">
          <Clock className="w-2.5 h-2.5 text-[#F3F724]" />
          <span className="text-[11px] font-mono font-semibold text-[#F3F724]">
            {formatDuration(data.total_duration_ms)}
          </span>
        </div>

        <div className="flex items-center gap-1 px-2 py-0.5 bg-[#141414] border border-[#1D1D1D] rounded">
          <Layers className="w-2.5 h-2.5 text-gray-400" />
          <span className="text-[10px] font-mono text-gray-400">{data.span_count} spans</span>
        </div>

        <div className="flex items-center gap-1 px-2 py-0.5 bg-[#141414] border border-[#1D1D1D] rounded">
          <span className="text-[10px] font-mono text-gray-400">{serviceCount} svc</span>
        </div>

        {hasErrors && (
          <div className="flex items-center gap-1 px-2 py-0.5 bg-[#EF4444]/8 border border-[#EF4444]/15 rounded">
            <AlertCircle className="w-2.5 h-2.5 text-[#EF4444]" />
            <span className="text-[10px] font-mono font-semibold text-[#EF4444]">
              {errorCount} err
            </span>
          </div>
        )}
      </div>

      {/* Service distribution bar */}
      {serviceList.length > 1 && (
        <div className="px-4 pb-2.5">
          <div className="flex h-1.5 rounded-full overflow-hidden bg-[#141414] border border-[#1D1D1D]">
            {serviceList.map((svc) => {
              const svcDuration = serviceDurations.get(svc) || 0
              const pct = (svcDuration / data.total_duration_ms) * 100

              return (
                <div
                  key={svc}
                  className="h-full transition-all duration-300"
                  style={{
                    width: `${Math.max(2, pct)}%`,
                    backgroundColor: getServiceColor(svc),
                    opacity: 0.8,
                  }}
                  title={`${svc}: ${pct.toFixed(1)}%`}
                />
              )
            })}
          </div>
          <div className="flex items-center gap-3 mt-1.5">
            {serviceList.map((svc) => (
              <div key={svc} className="flex items-center gap-1 text-[9px] text-gray-500">
                <div
                  className="w-1.5 h-1.5 rounded-full flex-shrink-0"
                  style={{ backgroundColor: getServiceColor(svc) }}
                />
                <span className="font-mono truncate">{svc}</span>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Critical path breadcrumb */}
      {criticalPath.length > 1 && (
        <div className="flex items-center gap-1 px-4 pb-2.5 overflow-x-auto">
          {criticalPath.map((span, i) => (
            <Fragment key={span.span_id}>
              {i > 0 && <ChevronRight className="w-3 h-3 text-gray-600 flex-shrink-0" />}
              <button
                type="button"
                onClick={() => onSpanClick?.(span)}
                className="text-[10px] font-mono text-gray-400 hover:text-white truncate max-w-[120px] flex-shrink-0 px-1 py-0.5 rounded hover:bg-[#1D1D1D] transition-colors"
                title={span.name}
              >
                {span.name}
              </button>
            </Fragment>
          ))}
        </div>
      )}
    </div>
  )
}
