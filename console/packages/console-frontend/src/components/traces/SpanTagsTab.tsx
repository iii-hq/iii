import { ChevronRight, Copy, Search } from 'lucide-react'
import { useMemo, useState } from 'react'
import type { VisualizationSpan } from '@/lib/traceTransform'
import { useCopyToClipboard } from '@/lib/traceUtils'

interface SpanTagsTabProps {
  span: VisualizationSpan
}

const NAMESPACE_LABELS: Record<string, string> = {
  http: 'HTTP',
  db: 'Database',
  rpc: 'RPC',
  net: 'Network',
  messaging: 'Messaging',
  faas: 'FaaS',
  enduser: 'End User',
  thread: 'Thread',
  code: 'Code',
  otel: 'OpenTelemetry',
  service: 'Service',
  telemetry: 'Telemetry',
  process: 'Process',
  os: 'OS',
  host: 'Host',
  container: 'Container',
  k8s: 'Kubernetes',
  cloud: 'Cloud',
  deployment: 'Deployment',
}

interface AttributeGroup {
  namespace: string
  label: string
  entries: [string, unknown][]
}

export function SpanTagsTab({ span }: SpanTagsTabProps) {
  const [searchQuery, setSearchQuery] = useState('')
  const { copiedKey, copy } = useCopyToClipboard()
  const [collapsedGroups, setCollapsedGroups] = useState<Set<string>>(new Set())

  const attributes = span.attributes || {}
  const entries = useMemo(() => Object.entries(attributes), [attributes])

  const filteredEntries = useMemo(() => {
    return entries.filter(([key, value]) => {
      if (!searchQuery) return true
      const query = searchQuery.toLowerCase()
      return key.toLowerCase().includes(query) || String(value).toLowerCase().includes(query)
    })
  }, [entries, searchQuery])

  const groups = useMemo(() => {
    const grouped = new Map<string, [string, unknown][]>()

    for (const [key, value] of filteredEntries) {
      const dotIndex = key.indexOf('.')
      const namespace = dotIndex > 0 ? key.substring(0, dotIndex) : '_other'

      if (!grouped.has(namespace)) {
        grouped.set(namespace, [])
      }
      grouped.get(namespace)?.push([key, value])
    }

    const result: AttributeGroup[] = []
    const sortedNamespaces = Array.from(grouped.keys()).sort((a, b) => {
      if (a === '_other') return 1
      if (b === '_other') return -1
      return a.localeCompare(b)
    })

    for (const ns of sortedNamespaces) {
      result.push({
        namespace: ns,
        label: ns === '_other' ? 'Other' : NAMESPACE_LABELS[ns] || ns,
        entries: grouped.get(ns) ?? [],
      })
    }

    return result
  }, [filteredEntries])

  const copyToClipboard = (key: string, value: unknown) => {
    const text = `${key}: ${typeof value === 'object' ? JSON.stringify(value) : String(value)}`
    copy(key, text)
  }

  const toggleGroup = (namespace: string) => {
    setCollapsedGroups((prev) => {
      const next = new Set(prev)
      if (next.has(namespace)) {
        next.delete(namespace)
      } else {
        next.add(namespace)
      }
      return next
    })
  }

  if (entries.length === 0) {
    return (
      <div className="p-8 text-center">
        <div className="w-10 h-10 mb-3 mx-auto rounded-lg bg-[#141414] border border-[#1D1D1D] flex items-center justify-center">
          <Search className="w-5 h-5 text-gray-600" />
        </div>
        <p className="text-sm text-gray-400">No attributes found</p>
        <p className="text-[11px] text-gray-600 mt-1">This span has no recorded attributes</p>
      </div>
    )
  }

  const showGrouped = groups.length > 1 && !searchQuery

  return (
    <div className="p-5 space-y-3">
      <div className="relative">
        <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-gray-500" />
        <input
          type="text"
          placeholder="Filter attributes..."
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          className="w-full pl-9 pr-4 py-2 bg-[#141414] border border-[#1D1D1D] rounded-md text-sm text-[#F4F4F4] placeholder-gray-600 focus:outline-none focus:border-[#F3F724]/50 transition-colors"
        />
      </div>

      {showGrouped ? (
        groups.map((group) => {
          const isCollapsed = collapsedGroups.has(group.namespace)

          return (
            <div
              key={group.namespace}
              className="bg-[#141414] rounded-lg border border-[#1D1D1D] overflow-hidden"
            >
              <button
                type="button"
                onClick={() => toggleGroup(group.namespace)}
                className="w-full flex items-center gap-2 px-4 py-2.5 hover:bg-[#1A1A1A] transition-colors text-left"
              >
                <ChevronRight
                  className={`w-3 h-3 text-gray-500 transition-transform ${isCollapsed ? '' : 'rotate-90'}`}
                />
                <span className="text-[11px] font-semibold text-gray-400 uppercase tracking-wider">
                  {group.label}
                </span>
                <span className="text-[10px] font-mono text-gray-600 ml-auto">
                  {group.entries.length}
                </span>
              </button>

              {!isCollapsed && (
                <div className="border-t border-[#1D1D1D] divide-y divide-[#1D1D1D]/50">
                  {group.entries.map(([key, value]) => (
                    <button
                      key={key}
                      type="button"
                      onClick={() => copyToClipboard(key, value)}
                      className="w-full px-4 py-2 hover:bg-[#1A1A1A] transition-colors text-left group"
                    >
                      <div className="flex items-start justify-between gap-3">
                        <div className="flex-1 min-w-0">
                          <div className="text-[11px] font-mono text-[#F3F724] mb-0.5">{key}</div>
                          <div className="text-[12px] text-gray-300 break-all font-mono leading-relaxed">
                            {typeof value === 'object'
                              ? JSON.stringify(value, null, 2)
                              : String(value)}
                          </div>
                        </div>
                        {copiedKey === key ? (
                          <span className="text-[10px] text-[#22C55E] flex-shrink-0 mt-0.5">
                            copied
                          </span>
                        ) : (
                          <Copy className="w-3 h-3 text-gray-600 opacity-0 group-hover:opacity-100 transition-opacity flex-shrink-0 mt-0.5" />
                        )}
                      </div>
                    </button>
                  ))}
                </div>
              )}
            </div>
          )
        })
      ) : (
        <div className="bg-[#141414] rounded-lg border border-[#1D1D1D] divide-y divide-[#1D1D1D]/50">
          {filteredEntries.map(([key, value]) => (
            <button
              key={key}
              type="button"
              onClick={() => copyToClipboard(key, value)}
              className="w-full px-4 py-2.5 hover:bg-[#1A1A1A] transition-colors text-left group"
            >
              <div className="flex items-start justify-between gap-3">
                <div className="flex-1 min-w-0">
                  <div className="text-[11px] font-mono text-[#F3F724] mb-0.5">{key}</div>
                  <div className="text-[12px] text-gray-300 break-all font-mono leading-relaxed">
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
      )}

      {filteredEntries.length === 0 && searchQuery && (
        <div className="text-center py-6">
          <p className="text-sm text-gray-400">No attributes match &ldquo;{searchQuery}&rdquo;</p>
        </div>
      )}

      <div className="text-[10px] text-gray-600 text-center pt-2">
        {filteredEntries.length} of {entries.length} attributes
      </div>
    </div>
  )
}
