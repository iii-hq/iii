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
                    <EventAttributeRow key={key} attrKey={key} value={value} />
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

/**
 * Render one event attribute row. When the value is a JSON-encoded string
 * (typically `iii.payload.json` written by the iii-sdk auto-capture), parse
 * and pretty-print it inside a <pre> block. Everything else renders as a
 * single-line label/value pair, matching the previous behavior.
 *
 * Keeps DOM cost low: pretty-printing only runs when the value clearly
 * looks like a JSON object/array (`{`/`[` prefix after trim) AND
 * JSON.parse succeeds. Strings that are bare numbers / booleans / plain
 * text fall through to the simple renderer.
 */
function EventAttributeRow({ attrKey, value }: { attrKey: string; value: unknown }) {
  const formatted = formatPossibleJson(value)
  if (formatted !== null) {
    return (
      <div className="flex flex-col gap-1 text-[11px]">
        <span className="text-gray-500 font-mono">{attrKey}</span>
        <pre className="text-gray-300 font-mono text-[10.5px] leading-snug bg-[#0A0A0A] border border-[#1D1D1D]/60 rounded-md px-3 py-2 overflow-x-auto whitespace-pre">
          {formatted}
        </pre>
      </div>
    )
  }
  return (
    <div className="flex items-start gap-2 text-[11px]">
      <span className="text-gray-500 font-mono flex-shrink-0">{attrKey}</span>
      <span className="text-gray-300 font-mono break-all">
        {typeof value === 'object' ? JSON.stringify(value) : String(value)}
      </span>
    </div>
  )
}

function formatPossibleJson(value: unknown): string | null {
  if (typeof value !== 'string') return null
  const trimmed = value.trim()
  if (trimmed.length === 0) return null
  const first = trimmed[0]
  // Quick filter: only attempt to parse strings that look like a JSON
  // object/array. Bare quoted strings (`"hi"`) are still valid JSON but
  // pretty-printing a single string adds no value and would just put
  // quotes around it.
  if (first !== '{' && first !== '[') return null
  try {
    const parsed = JSON.parse(trimmed)
    return JSON.stringify(parsed, null, 2)
  } catch {
    return null
  }
}
