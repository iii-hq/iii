import { Clock } from 'lucide-react'
import type { VisualizationSpan } from '@/lib/traceTransform'
import { toMs } from '@/lib/traceTransform'
import { formatRelative, formatTimestamp } from '@/lib/traceUtils'

interface SpanLogsTabProps {
  span: VisualizationSpan
}

export function SpanLogsTab({ span }: SpanLogsTabProps) {
  const sortedEvents = [...(span.events || [])].sort(
    (a, b) => a.timestamp_unix_nano - b.timestamp_unix_nano,
  )

  if (sortedEvents.length === 0) {
    return (
      <div className="p-8 text-center">
        <div className="w-10 h-10 mb-3 mx-auto rounded-lg bg-[#141414] border border-[#1D1D1D] flex items-center justify-center">
          <Clock className="w-5 h-5 text-gray-600" />
        </div>
        <p className="text-sm text-gray-400">No events recorded</p>
        <p className="text-[11px] text-gray-600 mt-1">
          This span has no logged events or exceptions
        </p>
      </div>
    )
  }

  const firstEventMs = toMs(sortedEvents[0].timestamp_unix_nano)

  return (
    <div className="p-5 space-y-2">
      {sortedEvents.map((event, index) => {
        const eventMs = toMs(event.timestamp_unix_nano)
        const offsetMs = eventMs - firstEventMs
        const isException = event.name === 'exception' || event.name?.startsWith('exception')
        const attrEntries = event.attributes ? Object.entries(event.attributes) : []

        return (
          <div
            key={`${event.name}-${event.timestamp_unix_nano}`}
            className={`rounded-lg border overflow-hidden ${
              isException ? 'bg-[#EF4444]/5 border-[#EF4444]/15' : 'bg-[#141414] border-[#1D1D1D]'
            }`}
          >
            <div className="px-4 py-3">
              <div className="flex items-start justify-between gap-3">
                <div className="flex-1 min-w-0">
                  <div
                    className={`text-sm font-semibold mb-0.5 ${
                      isException ? 'text-[#EF4444]' : 'text-[#F4F4F4]'
                    }`}
                  >
                    {event.name}
                  </div>
                  <div className="flex items-center gap-2 text-[10px] font-mono text-gray-500">
                    <span>{formatTimestamp(eventMs)}</span>
                    {index > 0 && (
                      <>
                        <span className="text-gray-600">&middot;</span>
                        <span className="text-gray-400">{formatRelative(offsetMs)}</span>
                      </>
                    )}
                  </div>
                </div>
                <span className="text-[10px] font-mono text-gray-600 flex-shrink-0 px-1.5 py-0.5 bg-[#0A0A0A] rounded">
                  #{index + 1}
                </span>
              </div>
            </div>

            {attrEntries.length > 0 && (
              <div className="border-t border-[#1D1D1D]/50 px-4 py-2.5">
                <div className="space-y-1.5">
                  {attrEntries.map(([key, value]) => (
                    <div key={key} className="flex items-start gap-2 text-[11px]">
                      <span className="text-gray-500 font-mono flex-shrink-0">{key}</span>
                      <span className="text-gray-300 font-mono break-all">
                        {typeof value === 'object' ? JSON.stringify(value) : String(value)}
                      </span>
                    </div>
                  ))}
                </div>
              </div>
            )}
          </div>
        )
      })}

      <div className="text-[10px] text-gray-600 text-center pt-2">
        {sortedEvents.length} event{sortedEvents.length !== 1 ? 's' : ''}
      </div>
    </div>
  )
}
