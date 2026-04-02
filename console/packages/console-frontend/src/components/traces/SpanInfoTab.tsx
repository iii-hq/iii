import { Copy } from 'lucide-react'
import type { VisualizationSpan, WaterfallData } from '@/lib/traceTransform'
import { formatDuration, getServiceName, useCopyToClipboard } from '@/lib/traceUtils'

interface SpanInfoTabProps {
  span: VisualizationSpan
  traceData: WaterfallData | null
}

export function SpanInfoTab({ span, traceData }: SpanInfoTabProps) {
  const { copiedKey: copiedField, copy: copyToClipboard } = useCopyToClipboard()

  const service = getServiceName(span)
  const tracePercent = traceData ? (span.duration_ms / traceData.total_duration_ms) * 100 : 0

  return (
    <div className="p-5 space-y-5">
      {/* Timing */}
      <div>
        <div className="text-[10px] font-semibold text-gray-500 uppercase tracking-wider mb-2.5">
          Timing
        </div>
        <div className="bg-[#141414] rounded-lg border border-[#1D1D1D] overflow-hidden">
          <div className="px-4 pt-4 pb-3">
            <div className="flex items-baseline justify-between mb-2">
              <span className="font-mono text-xl font-bold text-[#F3F724]">
                {formatDuration(span.duration_ms)}
              </span>
              {traceData && (
                <span className="text-[11px] font-mono text-gray-500">
                  {tracePercent.toFixed(1)}% of trace
                </span>
              )}
            </div>
            <div className="h-1.5 bg-[#0A0A0A] rounded-full overflow-hidden">
              <div
                className="h-full rounded-full transition-all duration-300"
                style={{
                  width: `${Math.min(100, Math.max(2, tracePercent))}%`,
                  background: 'linear-gradient(90deg, #F3F724, #D4D520)',
                }}
              />
            </div>
          </div>
          <div className="border-t border-[#1D1D1D] px-4 py-2.5 flex items-center justify-between">
            <span className="text-[11px] text-gray-500">Position in trace</span>
            <span className="text-[11px] font-mono text-gray-400">
              {span.start_percent.toFixed(1)}% →{' '}
              {Math.min(100, span.start_percent + span.width_percent).toFixed(1)}%
            </span>
          </div>
        </div>
      </div>

      {/* Status */}
      <div>
        <div className="text-[10px] font-semibold text-gray-500 uppercase tracking-wider mb-2.5">
          Status
        </div>
        <div className="bg-[#141414] rounded-lg border border-[#1D1D1D] px-4 py-3">
          <div className="flex items-center gap-2.5">
            <div
              className={`w-2.5 h-2.5 rounded-full ${
                span.status === 'ok'
                  ? 'bg-[#22C55E]'
                  : span.status === 'error'
                    ? 'bg-[#EF4444]'
                    : 'bg-gray-500'
              }`}
            />
            <span className="font-mono text-sm font-semibold text-[#F4F4F4]">
              {span.status.toUpperCase()}
            </span>
          </div>
        </div>
      </div>

      {/* Service & Operation */}
      <div>
        <div className="text-[10px] font-semibold text-gray-500 uppercase tracking-wider mb-2.5">
          Service & Operation
        </div>
        <div className="bg-[#141414] rounded-lg border border-[#1D1D1D] divide-y divide-[#1D1D1D]">
          <div className="px-4 py-2.5 flex items-center justify-between">
            <span className="text-[11px] text-gray-500">Service</span>
            <span className="text-sm text-[#F4F4F4] font-medium">{service}</span>
          </div>
          <div className="px-4 py-2.5 flex items-center justify-between gap-4">
            <span className="text-[11px] text-gray-500 flex-shrink-0">Operation</span>
            <span className="text-sm text-[#F4F4F4] font-mono truncate" title={span.name}>
              {span.name}
            </span>
          </div>
          {span.depth === 0 && (
            <div className="px-4 py-2.5 flex items-center justify-between">
              <span className="text-[11px] text-gray-500">Type</span>
              <span className="text-[11px] px-2 py-0.5 bg-[#F3F724]/10 text-[#F3F724] rounded font-medium">
                ROOT SPAN
              </span>
            </div>
          )}
        </div>
      </div>

      {/* Identifiers */}
      <div>
        <div className="text-[10px] font-semibold text-gray-500 uppercase tracking-wider mb-2.5">
          Identifiers
        </div>
        <div className="bg-[#141414] rounded-lg border border-[#1D1D1D] divide-y divide-[#1D1D1D]">
          {[
            { label: 'Trace ID', value: span.trace_id, field: 'traceId' },
            { label: 'Span ID', value: span.span_id, field: 'spanId' },
            ...(span.parent_span_id
              ? [{ label: 'Parent Span', value: span.parent_span_id, field: 'parentId' }]
              : []),
          ].map(({ label, value, field }) => (
            <button
              key={field}
              type="button"
              onClick={() => copyToClipboard(field, value)}
              className="w-full px-4 py-2.5 flex items-center justify-between hover:bg-[#1A1A1A] transition-colors group text-left"
            >
              <span className="text-[11px] text-gray-500 flex-shrink-0">{label}</span>
              <div className="flex items-center gap-2 min-w-0 ml-4">
                <span className="text-[11px] font-mono text-gray-400 truncate" title={value}>
                  {value}
                </span>
                {copiedField === field ? (
                  <span className="text-[10px] text-[#22C55E] flex-shrink-0">copied</span>
                ) : (
                  <Copy className="w-3 h-3 text-gray-600 opacity-0 group-hover:opacity-100 transition-opacity flex-shrink-0" />
                )}
              </div>
            </button>
          ))}
        </div>
      </div>

      {/* Hierarchy */}
      <div>
        <div className="text-[10px] font-semibold text-gray-500 uppercase tracking-wider mb-2.5">
          Hierarchy
        </div>
        <div className="bg-[#141414] rounded-lg border border-[#1D1D1D] divide-y divide-[#1D1D1D]">
          <div className="px-4 py-2.5 flex items-center justify-between">
            <span className="text-[11px] text-gray-500">Depth</span>
            <span className="text-sm font-mono text-[#F4F4F4]">{span.depth}</span>
          </div>
          {span.flags !== undefined && (
            <div className="px-4 py-2.5 flex items-center justify-between">
              <span className="text-[11px] text-gray-500">Flags</span>
              <span className="text-sm font-mono text-gray-400">0x{span.flags.toString(16)}</span>
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
