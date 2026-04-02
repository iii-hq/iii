import { Copy, Package } from 'lucide-react'
import { useMemo } from 'react'
import type { VisualizationSpan } from '@/lib/traceTransform'
import { useCopyToClipboard } from '@/lib/traceUtils'

interface SpanBaggageTabProps {
  span: VisualizationSpan
}

export function SpanBaggageTab({ span }: SpanBaggageTabProps) {
  const { copiedKey, copy } = useCopyToClipboard()

  const baggageEntries = useMemo(
    () =>
      Object.entries(span.attributes || {})
        .filter(([key]) => key.startsWith('baggage.'))
        .map(([key, value]) => [key.replace('baggage.', ''), value] as [string, unknown]),
    [span.attributes],
  )

  if (baggageEntries.length === 0) {
    return (
      <div className="p-8 text-center">
        <div className="w-10 h-10 mb-3 mx-auto rounded-lg bg-[#141414] border border-[#1D1D1D] flex items-center justify-center">
          <Package className="w-5 h-5 text-gray-600" />
        </div>
        <p className="text-sm text-gray-400">No baggage context</p>
        <p className="text-[11px] text-gray-600 mt-1 max-w-xs mx-auto">
          W3C Baggage propagates key-value pairs across service boundaries. None was attached to
          this span.
        </p>
      </div>
    )
  }

  return (
    <div className="p-5 space-y-3">
      <div className="flex items-center gap-2 px-1">
        <Package className="w-3.5 h-3.5 text-[#F3F724]" />
        <span className="text-[11px] font-semibold text-gray-400 uppercase tracking-wider">
          W3C Baggage Context
        </span>
      </div>

      <div className="bg-[#141414] rounded-lg border border-[#1D1D1D] divide-y divide-[#1D1D1D]/50">
        {baggageEntries.map(([key, value]) => (
          <button
            key={key}
            type="button"
            onClick={() =>
              copy(
                key,
                `${key}: ${typeof value === 'object' ? JSON.stringify(value) : String(value)}`,
              )
            }
            aria-label={`Copy ${key} to clipboard`}
            className="w-full px-4 py-2.5 hover:bg-[#1A1A1A] transition-colors text-left group"
          >
            <div className="flex items-start justify-between gap-3">
              <div className="flex-1 min-w-0">
                <div className="text-[11px] font-mono text-[#F3F724] mb-0.5">{key}</div>
                <div className="text-[12px] text-gray-300 break-all font-mono">
                  {typeof value === 'object' ? JSON.stringify(value, null, 2) : String(value)}
                </div>
              </div>
              {copiedKey === key ? (
                <span className="text-[10px] text-[#22C55E] flex-shrink-0 mt-0.5">copied</span>
              ) : (
                <Copy className="w-3 h-3 text-gray-600 opacity-0 group-hover:opacity-100 transition-opacity flex-shrink-0 mt-0.5" />
              )}
            </div>
          </button>
        ))}
      </div>

      <div className="text-[10px] text-gray-600 text-center pt-2">
        {baggageEntries.length} baggage item{baggageEntries.length !== 1 ? 's' : ''}
      </div>
    </div>
  )
}
