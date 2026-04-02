import { ExternalLink } from 'lucide-react'
import type { VisualizationSpan } from '@/lib/traceTransform'

interface SpanLinksTabProps {
  span: VisualizationSpan
  onNavigateToTrace?: (traceId: string) => void
}

export function SpanLinksTab({ span, onNavigateToTrace }: SpanLinksTabProps) {
  const links = span.links || []

  if (links.length === 0) {
    return <div className="p-4 text-center text-xs text-gray-500">No linked traces</div>
  }

  return (
    <div className="p-3 space-y-2">
      <div className="text-[10px] text-gray-500 uppercase tracking-wider mb-2">
        Linked Traces ({links.length})
      </div>
      {links.map((link, i) => (
        <button
          key={`${link.trace_id}-${link.span_id}-${i}`}
          type="button"
          onClick={() => onNavigateToTrace?.(link.trace_id)}
          className="w-full flex items-center gap-2 px-3 py-2 bg-[#141414] border border-[#1D1D1D] rounded hover:bg-[#1A1A1A] hover:border-[#252525] transition-colors text-left group"
        >
          <ExternalLink className="w-3 h-3 text-gray-500 group-hover:text-[#F3F724] flex-shrink-0" />
          <div className="flex-1 min-w-0">
            <div className="text-[11px] font-mono text-gray-300 truncate group-hover:text-[#F4F4F4]">
              {link.trace_id.slice(0, 16)}...
            </div>
            <div className="text-[10px] font-mono text-gray-600">
              span: {link.span_id.slice(0, 12)}
            </div>
          </div>
          {link.attributes && Object.keys(link.attributes).length > 0 && (
            <span className="text-[9px] text-gray-600 bg-[#1D1D1D] px-1.5 py-0.5 rounded flex-shrink-0">
              +{Object.keys(link.attributes).length} attrs
            </span>
          )}
        </button>
      ))}
    </div>
  )
}
