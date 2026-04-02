import { ChevronRight } from 'lucide-react'
import { useMemo, useState } from 'react'
import { getServiceColor } from '@/lib/traceColors'
import type { WaterfallData } from '@/lib/traceTransform'
import { formatDuration, getServiceName } from '@/lib/traceUtils'

interface ServiceBreakdownProps {
  data: WaterfallData
}

interface ServiceStats {
  name: string
  color: string
  spanCount: number
  totalDuration: number
  errorCount: number
  percentage: number
}

function calculatePercentile(values: number[], percentile: number): number {
  if (values.length === 0) return 0
  const sorted = [...values].sort((a, b) => a - b)
  const index = Math.ceil((percentile / 100) * sorted.length) - 1
  return sorted[Math.max(0, index)]
}

export function ServiceBreakdown({ data }: ServiceBreakdownProps) {
  const [isExpanded, setIsExpanded] = useState(true)

  const { serviceStats, percentiles } = useMemo(() => {
    const statsMap = new Map<string, ServiceStats>()
    const durations: number[] = []

    for (const span of data.spans) {
      const serviceName = getServiceName(span)

      if (Number.isFinite(span.duration_ms)) {
        durations.push(span.duration_ms)
      }

      if (!statsMap.has(serviceName)) {
        statsMap.set(serviceName, {
          name: serviceName,
          color: getServiceColor(serviceName),
          spanCount: 0,
          totalDuration: 0,
          errorCount: 0,
          percentage: 0,
        })
      }

      const stats = statsMap.get(serviceName)
      if (stats) {
        stats.spanCount++
        stats.totalDuration += Number.isFinite(span.duration_ms) ? span.duration_ms : 0
        if (span.status === 'error') stats.errorCount++
      }
    }

    const totalDuration = data.total_duration_ms
    for (const stats of statsMap.values()) {
      stats.percentage = (stats.totalDuration / totalDuration) * 100
    }

    const serviceStats = Array.from(statsMap.values()).sort(
      (a, b) => b.totalDuration - a.totalDuration,
    )

    const percentiles = {
      p50: calculatePercentile(durations, 50),
      p95: calculatePercentile(durations, 95),
      p99: calculatePercentile(durations, 99),
      max: durations.length > 0 ? Math.max(...durations) : 0,
    }

    return { serviceStats, percentiles }
  }, [data])

  return (
    <div className="bg-[#0A0A0A]">
      {/* Collapsible header */}
      <button
        type="button"
        onClick={() => setIsExpanded(!isExpanded)}
        className="w-full flex items-center gap-2 px-4 py-2.5 hover:bg-[#141414] transition-colors text-left"
        aria-label="Toggle services section"
        aria-expanded={isExpanded}
      >
        <ChevronRight
          className={`w-3 h-3 text-gray-500 transition-transform duration-150 ${isExpanded ? 'rotate-90' : ''}`}
        />
        <span className="text-[10px] font-semibold text-gray-400 uppercase tracking-wider">
          Services
        </span>

        {/* Inline percentile summary — always visible */}
        <div className="flex items-center gap-3 ml-auto text-[10px] font-mono text-gray-500">
          <span>
            p50 <span className="text-gray-300">{formatDuration(percentiles.p50)}</span>
          </span>
          <span>
            p95 <span className="text-gray-300">{formatDuration(percentiles.p95)}</span>
          </span>
          <span>
            p99 <span className="text-gray-300">{formatDuration(percentiles.p99)}</span>
          </span>
        </div>
      </button>

      {isExpanded && (
        <div className="px-4 pb-3 space-y-2.5">
          {/* Service rows — each with name, bar, stats */}
          {serviceStats.map((service) => (
            <div key={service.name} className="group">
              <div className="flex items-center gap-2 mb-1">
                <div
                  className="w-2 h-2 rounded-sm flex-shrink-0"
                  style={{ backgroundColor: service.color }}
                />
                <span className="text-[11px] text-[#F4F4F4] truncate flex-1 font-medium">
                  {service.name}
                </span>
                <span className="text-[10px] font-mono text-gray-500">
                  {service.spanCount} span{service.spanCount !== 1 ? 's' : ''}
                </span>
                <span className="text-[10px] font-mono text-gray-300 w-16 text-right">
                  {formatDuration(service.totalDuration)}
                </span>
                {service.errorCount > 0 && (
                  <span className="text-[10px] font-mono text-[#EF4444] font-semibold">
                    {service.errorCount} err
                  </span>
                )}
              </div>
              {/* Proportional bar */}
              <div className="h-1 rounded-full bg-[#141414] overflow-hidden ml-4">
                <div
                  className="h-full rounded-full transition-all duration-300"
                  style={{
                    width: `${Math.max(2, service.percentage)}%`,
                    backgroundColor: service.color,
                    opacity: 0.7,
                  }}
                />
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
