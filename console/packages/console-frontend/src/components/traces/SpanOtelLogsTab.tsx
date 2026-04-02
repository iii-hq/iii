import { useQuery } from '@tanstack/react-query'
import { ChevronDown, ChevronRight, Copy, FileText } from 'lucide-react'
import { useMemo, useState } from 'react'
import { fetchOtelLogs } from '@/api'
import type { OtelLog } from '@/api/observability/logs'
import type { VisualizationSpan } from '@/lib/traceTransform'
import { toMs } from '@/lib/traceTransform'
import { formatRelative, formatTimestamp, useCopyToClipboard } from '@/lib/traceUtils'

interface SpanOtelLogsTabProps {
  span: VisualizationSpan
}

const SEVERITY_STYLES = {
  error: {
    label: 'ERROR',
    text: 'text-[#EF4444]',
    badge: 'bg-[#EF4444]/15 text-[#EF4444] border-[#EF4444]/20',
    accent: '#EF4444',
    cardBg: 'bg-[#EF4444]/5',
  },
  warn: {
    label: 'WARN',
    text: 'text-[#F59E0B]',
    badge: 'bg-[#F59E0B]/15 text-[#F59E0B] border-[#F59E0B]/20',
    accent: '#F59E0B',
    cardBg: 'bg-[#F59E0B]/5',
  },
  info: {
    label: 'INFO',
    text: 'text-[#3B82F6]',
    badge: 'bg-[#3B82F6]/15 text-[#3B82F6] border-[#3B82F6]/20',
    accent: '#3B82F6',
    cardBg: 'bg-[#141414]',
  },
  debug: {
    label: 'DEBUG',
    text: 'text-gray-400',
    badge: 'bg-gray-500/10 text-gray-400 border-gray-500/20',
    accent: '#6B7280',
    cardBg: 'bg-[#141414]',
  },
} as const

function getSeverity(severityText: string) {
  const level = severityText?.toUpperCase() || ''
  if (level.startsWith('ERROR') || level.startsWith('FATAL'))
    return { ...SEVERITY_STYLES.error, label: level || 'ERROR' }
  if (level.startsWith('WARN')) return { ...SEVERITY_STYLES.warn, label: level || 'WARN' }
  if (level.startsWith('INFO')) return { ...SEVERITY_STYLES.info, label: level || 'INFO' }
  return { ...SEVERITY_STYLES.debug, label: level || 'DEBUG' }
}

// IDs already shown in the span header - don't duplicate
const HIDDEN_ATTRS = new Set(['span_id', 'trace_id'])

function isJsonString(str: string): boolean {
  if (typeof str !== 'string') return false
  const trimmed = str.trim()
  return (
    (trimmed.startsWith('{') && trimmed.endsWith('}')) ||
    (trimmed.startsWith('[') && trimmed.endsWith(']'))
  )
}

function tryParseJson(value: unknown): { isJson: boolean; parsed: unknown; raw: string } {
  const raw = typeof value === 'object' ? JSON.stringify(value) : String(value)
  if (typeof value === 'object' && value !== null) {
    return { isJson: true, parsed: value, raw }
  }
  if (typeof value === 'string' && isJsonString(value)) {
    try {
      return { isJson: true, parsed: JSON.parse(value), raw }
    } catch {
      /* not valid JSON */
    }
  }
  return { isJson: false, parsed: value, raw }
}

function JsonValue({ value }: { value: unknown }) {
  const [expanded, setExpanded] = useState(false)
  const { isJson, parsed, raw } = tryParseJson(value)

  if (!isJson) {
    return <span className="text-gray-300 font-mono break-all text-[11px]">{raw}</span>
  }

  const pretty = JSON.stringify(parsed, null, 2)
  const lineCount = pretty.split('\n').length

  if (lineCount <= 2) {
    return <span className="text-gray-300 font-mono break-all text-[11px]">{raw}</span>
  }

  return (
    <div className="flex-1 min-w-0">
      <button
        type="button"
        onClick={() => setExpanded(!expanded)}
        className="flex items-center gap-1 text-[10px] text-gray-500 hover:text-gray-300 transition-colors cursor-pointer"
      >
        {expanded ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
        <span className="font-mono">{expanded ? 'collapse' : `${lineCount} lines`}</span>
      </button>
      {expanded ? (
        <pre className="mt-1.5 p-2.5 bg-[#0A0A0A] border border-[#1D1D1D] rounded text-[10px] font-mono text-gray-300 overflow-x-auto leading-relaxed whitespace-pre-wrap break-all">
          {pretty}
        </pre>
      ) : (
        <div className="mt-1 text-[11px] font-mono text-gray-400 truncate">{raw}</div>
      )}
    </div>
  )
}

function CopyableValue({ label, value }: { label: string; value: string }) {
  const { copiedKey, copy } = useCopyToClipboard()
  const isCopied = copiedKey === label

  return (
    <button
      type="button"
      onClick={() => copy(label, value)}
      className="flex items-center gap-1 text-[11px] font-mono text-gray-400 hover:text-gray-200 transition-colors group cursor-pointer"
    >
      <span className="truncate">{value}</span>
      {isCopied ? (
        <span className="text-[#22C55E] text-[9px] flex-shrink-0">copied</span>
      ) : (
        <Copy className="w-2.5 h-2.5 opacity-0 group-hover:opacity-100 transition-opacity flex-shrink-0" />
      )}
    </button>
  )
}

function LogCard({ log, index, firstLogMs }: { log: OtelLog; index: number; firstLogMs: number }) {
  const logMs = toMs(log.timestamp_unix_nano)
  const offsetMs = logMs - firstLogMs
  const severity = getSeverity(log.severity_text)

  const dataAttrs: Array<[string, unknown]> = []
  const metaAttrs: Array<[string, unknown]> = []

  if (log.attributes) {
    for (const [key, value] of Object.entries(log.attributes)) {
      if (HIDDEN_ATTRS.has(key)) continue
      const { isJson } = tryParseJson(value)
      if (isJson || (typeof value === 'string' && value.length > 80)) {
        dataAttrs.push([key, value])
      } else {
        metaAttrs.push([key, value])
      }
    }
  }

  return (
    <div className={`rounded-lg border border-[#1D1D1D] overflow-hidden ${severity.cardBg}`}>
      {/* Severity accent + message */}
      <div className="flex">
        <div
          className="w-1 flex-shrink-0 rounded-l-lg"
          style={{ backgroundColor: severity.accent }}
        />

        <div className="flex-1 min-w-0 px-3.5 py-3">
          {/* Header: message + severity badge */}
          <div className="flex items-start justify-between gap-2 mb-1.5">
            <p className={`text-[13px] font-medium leading-snug ${severity.text}`}>{log.body}</p>
            <span
              className={`text-[9px] font-mono font-semibold uppercase tracking-wider flex-shrink-0 px-1.5 py-0.5 rounded border ${severity.badge}`}
            >
              {severity.label}
            </span>
          </div>

          {/* Timestamp row */}
          <div className="flex items-center gap-2 text-[10px] font-mono text-gray-500 mb-0.5">
            <span className="text-gray-400">{formatTimestamp(logMs)}</span>
            {index > 0 && <span className="text-gray-500">{formatRelative(offsetMs)}</span>}
            {log.service_name && (
              <>
                <span className="text-gray-600">&middot;</span>
                <span className="px-1 py-0.5 bg-[#1D1D1D] rounded text-gray-400">
                  {log.service_name}
                </span>
              </>
            )}
          </div>

          {/* Inline meta attributes (short values) */}
          {metaAttrs.length > 0 && (
            <div className="flex flex-wrap items-center gap-x-3 gap-y-1 mt-2.5 pt-2.5 border-t border-[#1D1D1D]/50">
              {metaAttrs.map(([key, value]) => (
                <div key={key} className="flex items-center gap-1.5 text-[10px]">
                  <span className="text-gray-600 font-mono">{key}</span>
                  <CopyableValue label={key} value={String(value)} />
                </div>
              ))}
            </div>
          )}

          {/* Expandable data attributes (JSON / long values) */}
          {dataAttrs.length > 0 && (
            <div className="mt-2.5 pt-2.5 border-t border-[#1D1D1D]/50 space-y-2">
              {dataAttrs.map(([key, value]) => (
                <div key={key}>
                  <span className="text-[10px] text-gray-600 font-mono">{key}</span>
                  <div className="mt-0.5">
                    <JsonValue value={value} />
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  )
}

export function SpanOtelLogsTab({ span }: SpanOtelLogsTabProps) {
  const { data, isLoading } = useQuery({
    queryKey: ['span-otel-logs', span.trace_id, span.span_id],
    queryFn: () => fetchOtelLogs({ trace_id: span.trace_id ?? undefined, span_id: span.span_id }),
    enabled: !!span.trace_id && !!span.span_id,
  })

  const logs: OtelLog[] = data?.logs ?? []

  const { errorCount, warnCount } = useMemo(() => {
    let errorCount = 0
    let warnCount = 0
    for (const l of logs) {
      const sev = l.severity_text?.toUpperCase() ?? ''
      if (sev.startsWith('ERROR')) errorCount++
      else if (sev.startsWith('WARN')) warnCount++
    }
    return { errorCount, warnCount }
  }, [logs])

  if (isLoading) {
    return (
      <div className="p-8 text-center">
        <div className="w-5 h-5 mx-auto mb-2 border-2 border-gray-600 border-t-[#F3F724] rounded-full animate-spin" />
        <p className="text-[11px] text-gray-500">Loading logs...</p>
      </div>
    )
  }

  if (logs.length === 0) {
    return (
      <div className="p-8 text-center">
        <div className="w-10 h-10 mb-3 mx-auto rounded-lg bg-[#141414] border border-[#1D1D1D] flex items-center justify-center">
          <FileText className="w-5 h-5 text-gray-600" />
        </div>
        <p className="text-sm text-gray-400">No logs found</p>
        <p className="text-[11px] text-gray-600 mt-1">No log entries correlated with this span</p>
      </div>
    )
  }

  const firstLogMs = toMs(logs[0].timestamp_unix_nano)

  return (
    <div className="p-4 space-y-2.5">
      {/* Summary bar */}
      <div className="flex items-center justify-between px-1 mb-1">
        <span className="text-[10px] text-gray-500 font-mono">
          {logs.length} log{logs.length !== 1 ? 's' : ''} correlated
        </span>
        <div className="flex items-center gap-2">
          {errorCount > 0 && (
            <span className="flex items-center gap-1 text-[9px] font-mono text-[#EF4444]">
              <span className="w-1.5 h-1.5 rounded-full bg-[#EF4444]" />
              {errorCount} errors
            </span>
          )}
          {warnCount > 0 && (
            <span className="flex items-center gap-1 text-[9px] font-mono text-[#F59E0B]">
              <span className="w-1.5 h-1.5 rounded-full bg-[#F59E0B]" />
              {warnCount} warnings
            </span>
          )}
        </div>
      </div>

      {logs.map((log, index) => (
        <LogCard
          key={`log-${log.timestamp_unix_nano}-${index}`}
          log={log}
          index={index}
          firstLogMs={firstLogMs}
        />
      ))}
    </div>
  )
}
