import { ChevronRight } from 'lucide-react'
import { useEffect, useMemo, useReducer, useRef, useState } from 'react'
import type { VisualizationSpan, WaterfallData } from '@/lib/traceTransform'
import { formatDuration } from '@/lib/traceUtils'

interface WaterfallChartProps {
  data: WaterfallData
  onSpanClick: (span: VisualizationSpan) => void
  selectedSpanId?: string | null
}

interface SpanNode extends VisualizationSpan {
  children: SpanNode[]
  isExpanded: boolean
  isCriticalPath: boolean
}

function buildSpanTree(spans: VisualizationSpan[]): SpanNode[] {
  const spanMap = new Map<string, SpanNode>()
  const roots: SpanNode[] = []

  spans.forEach((span) => {
    spanMap.set(span.span_id, {
      ...span,
      children: [],
      isExpanded: true,
      isCriticalPath: false,
    })
  })

  spans.forEach((span) => {
    const node = spanMap.get(span.span_id)
    if (!node) return
    if (span.parent_span_id && spanMap.has(span.parent_span_id)) {
      spanMap.get(span.parent_span_id)?.children.push(node)
    } else {
      roots.push(node)
    }
  })

  function markCriticalPath(node: SpanNode): number {
    if (node.children.length === 0) {
      node.isCriticalPath = true
      return node.duration_ms
    }

    let maxDuration = 0
    let criticalChild: SpanNode | null = null

    node.children.forEach((child) => {
      const duration = markCriticalPath(child)
      if (duration > maxDuration) {
        maxDuration = duration
        criticalChild = child
      }
    })

    node.isCriticalPath = true
    node.children.forEach((child) => {
      if (child !== criticalChild) {
        unmarkCriticalPath(child)
      }
    })

    return node.duration_ms + maxDuration
  }

  function unmarkCriticalPath(node: SpanNode) {
    node.isCriticalPath = false
    node.children.forEach(unmarkCriticalPath)
  }

  roots.forEach(markCriticalPath)

  return roots
}

function flattenTree(nodes: SpanNode[], expandedIds: Set<string>): SpanNode[] {
  const result: SpanNode[] = []

  function traverse(node: SpanNode) {
    result.push(node)
    if (expandedIds.has(node.span_id)) {
      node.children.forEach(traverse)
    }
  }

  nodes.forEach(traverse)
  return result
}

interface DisplayState {
  expandedIds: Set<string>
  showCriticalPath: boolean
  hoveredSpanId: string | null
  scrollPosition: number
}

type DisplayAction =
  | { type: 'TOGGLE_SPAN'; spanId: string }
  | { type: 'SET_ALL_EXPANDED'; ids: Set<string> }
  | { type: 'SET_CRITICAL_PATH'; value: boolean }
  | { type: 'SET_HOVERED_SPAN'; spanId: string | null }
  | { type: 'SET_SCROLL'; position: number }

const initialDisplayState: DisplayState = {
  expandedIds: new Set(),
  showCriticalPath: false,
  hoveredSpanId: null,
  scrollPosition: 0,
}

function displayReducer(state: DisplayState, action: DisplayAction): DisplayState {
  switch (action.type) {
    case 'TOGGLE_SPAN': {
      const next = new Set(state.expandedIds)
      next.has(action.spanId) ? next.delete(action.spanId) : next.add(action.spanId)
      return { ...state, expandedIds: next }
    }
    case 'SET_ALL_EXPANDED':
      return { ...state, expandedIds: action.ids }
    case 'SET_CRITICAL_PATH':
      return { ...state, showCriticalPath: action.value }
    case 'SET_HOVERED_SPAN':
      return { ...state, hoveredSpanId: action.spanId }
    case 'SET_SCROLL':
      return { ...state, scrollPosition: action.position }
  }
}

export function WaterfallChart({ data, onSpanClick, selectedSpanId }: WaterfallChartProps) {
  const [displayState, dispatch] = useReducer(displayReducer, initialDisplayState)
  const { expandedIds, showCriticalPath, hoveredSpanId, scrollPosition } = displayState
  const containerRef = useRef<HTMLDivElement>(null)

  // Span column resize
  const [spanColWidth, setSpanColWidth] = useState(() => {
    const saved = localStorage.getItem('iii-span-col-width')
    return saved ? Number.parseInt(saved, 10) : 300
  })
  const colResizeRef = useRef<{ startX: number; startWidth: number } | null>(null)
  const spanColWidthRef = useRef(spanColWidth)
  spanColWidthRef.current = spanColWidth

  useEffect(() => {
    localStorage.setItem('iii-span-col-width', String(spanColWidth))
  }, [spanColWidth])

  useEffect(() => {
    let rafId: number | null = null
    const onMouseMove = (e: MouseEvent) => {
      if (!colResizeRef.current) return
      if (rafId !== null) return
      rafId = requestAnimationFrame(() => {
        rafId = null
        if (!colResizeRef.current) return
        const diff = e.clientX - colResizeRef.current.startX
        setSpanColWidth(Math.min(Math.max(colResizeRef.current.startWidth + diff, 150), 600))
      })
    }
    const onMouseUp = () => {
      if (!colResizeRef.current) return
      colResizeRef.current = null
      if (rafId !== null) {
        cancelAnimationFrame(rafId)
        rafId = null
      }
      document.body.style.cursor = ''
      document.body.style.userSelect = ''
    }
    window.addEventListener('mousemove', onMouseMove)
    window.addEventListener('mouseup', onMouseUp)
    return () => {
      window.removeEventListener('mousemove', onMouseMove)
      window.removeEventListener('mouseup', onMouseUp)
      if (rafId !== null) cancelAnimationFrame(rafId)
    }
  }, [])

  const startColResize = (e: React.MouseEvent) => {
    e.preventDefault()
    colResizeRef.current = { startX: e.clientX, startWidth: spanColWidthRef.current }
    document.body.style.cursor = 'col-resize'
    document.body.style.userSelect = 'none'
  }

  const totalMs = data.total_duration_ms || 1
  const rulerMarks = useMemo(
    () =>
      [0, 25, 50, 75, 100].map((pct) => ({
        pct,
        label: formatDuration((totalMs * pct) / 100),
      })),
    [totalMs],
  )

  const spanTree = useMemo(() => buildSpanTree(data.spans), [data.spans])

  useEffect(() => {
    const allIds = new Set(data.spans.map((s) => s.span_id))
    dispatch({ type: 'SET_ALL_EXPANDED', ids: allIds })
  }, [data.spans])

  const visibleSpans = useMemo(() => flattenTree(spanTree, expandedIds), [spanTree, expandedIds])

  const handleScroll = (e: React.UIEvent<HTMLDivElement>) => {
    dispatch({ type: 'SET_SCROLL', position: e.currentTarget.scrollTop })
  }

  const toggleExpand = (spanId: string) => {
    dispatch({ type: 'TOGGLE_SPAN', spanId })
  }

  const expandAll = () => {
    const allIds = new Set(data.spans.map((s) => s.span_id))
    dispatch({ type: 'SET_ALL_EXPANDED', ids: allIds })
  }

  const collapseAll = () => {
    dispatch({ type: 'SET_ALL_EXPANDED', ids: new Set() })
  }

  const miniMapHeight = 80
  const contentHeight = visibleSpans.length * 32
  const viewportRatio = containerRef.current ? containerRef.current.clientHeight / contentHeight : 1
  const thumbHeight = Math.max(20, miniMapHeight * viewportRatio)
  const thumbPosition =
    containerRef.current && contentHeight > 0 ? (scrollPosition / contentHeight) * miniMapHeight : 0

  return (
    <div className="flex flex-col h-full">
      <div className="flex items-center justify-between px-3 py-2 border-b border-[#1D1D1D] bg-[#141414]/30">
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={expandAll}
            className="px-2 py-1 text-xs text-gray-400 hover:text-[#F4F4F4] hover:bg-[#1D1D1D] rounded transition-colors"
          >
            Expand all
          </button>
          <button
            type="button"
            onClick={collapseAll}
            className="px-2 py-1 text-xs text-gray-400 hover:text-[#F4F4F4] hover:bg-[#1D1D1D] rounded transition-colors"
          >
            Collapse all
          </button>
          <div className="w-px h-4 bg-[#1D1D1D] mx-1" />
          <label className="flex items-center gap-2 text-xs text-gray-400 cursor-pointer">
            <input
              type="checkbox"
              checked={showCriticalPath}
              onChange={(e) => dispatch({ type: 'SET_CRITICAL_PATH', value: e.target.checked })}
              className="rounded border-[#1D1D1D] bg-[#141414] text-[#F3F724] focus:ring-[#F3F724]/30"
            />
            Show critical path
          </label>
        </div>
        <div className="text-xs text-gray-400">
          {visibleSpans.length} of {data.span_count} spans
        </div>
      </div>

      <div
        className="grid gap-4 px-3 py-2 text-[11px] font-semibold text-gray-400 uppercase tracking-wide border-b border-[#1D1D1D] bg-[#141414]/50"
        style={{ gridTemplateColumns: `${spanColWidth}px 1fr` }}
      >
        <div className="flex items-center relative">
          <span>Span</span>
          <div
            role="slider"
            aria-label="Resize span column"
            aria-orientation="horizontal"
            aria-valuemin={150}
            aria-valuemax={600}
            aria-valuenow={spanColWidth}
            tabIndex={0}
            onMouseDown={startColResize}
            onDoubleClick={() => setSpanColWidth(300)}
            className="absolute right-[-11px] top-0 bottom-0 w-[7px] cursor-col-resize z-10 group"
            title="Drag to resize, double-click to reset"
          >
            <div className="absolute left-[3px] top-0 bottom-0 w-[1px] bg-[#1D1D1D] group-hover:bg-accent/50 transition-colors" />
          </div>
        </div>
        <div className="flex justify-between">
          {rulerMarks.map(({ pct, label }) => (
            <span key={pct} className="font-mono">
              {label}
            </span>
          ))}
        </div>
      </div>

      <div className="flex flex-1 overflow-hidden">
        <div ref={containerRef} className="flex-1 overflow-y-auto" onScroll={handleScroll}>
          {visibleSpans.map((span) => {
            const hasChildren = span.children.length > 0
            const isExpanded = expandedIds.has(span.span_id)
            const isCritical = showCriticalPath && span.isCriticalPath
            const isSelected = selectedSpanId === span.span_id
            const isHovered = hoveredSpanId === span.span_id

            const getBarStyle = (): React.CSSProperties => {
              if (isCritical) return { background: 'linear-gradient(to right, #F97316, #FB923C)' }
              if (span.status === 'error')
                return { background: 'linear-gradient(to right, #EF4444, #DC2626)' }
              if (span.status === 'ok')
                return { background: 'linear-gradient(to right, #22C55E, #16A34A)' }
              return { background: 'linear-gradient(to right, #6B7280, #4B5563)' }
            }

            const statusColors = {
              ok: '#22C55E',
              error: '#EF4444',
              unset: '#6B7280',
            }

            const barStyle = getBarStyle()

            return (
              <button
                key={span.span_id}
                type="button"
                className={`
                  grid gap-4 px-3 py-1 items-center transition-colors cursor-pointer w-full text-left
                  ${isSelected ? 'bg-[#F3F724]/[0.06] border-l-2 border-l-[#F3F724]' : isHovered ? 'bg-[#1D1D1D]' : 'hover:bg-[#1D1D1D]/50'}
                  ${isCritical && !isSelected ? 'bg-orange-500/5' : ''}
                `}
                style={{ gridTemplateColumns: `${spanColWidth}px 1fr` }}
                onClick={() => onSpanClick(span)}
                onMouseEnter={() => dispatch({ type: 'SET_HOVERED_SPAN', spanId: span.span_id })}
                onMouseLeave={() => dispatch({ type: 'SET_HOVERED_SPAN', spanId: null })}
              >
                <div className="flex items-center gap-1.5 min-w-0">
                  <div className="flex-shrink-0 flex" style={{ width: span.depth * 16 }}>
                    {Array.from({ length: span.depth }).map((_, i) => (
                      <div
                        key={`${span.span_id}-indent-${i}`}
                        className="w-4 h-6 border-l border-[#1D1D1D]/50"
                      />
                    ))}
                  </div>

                  {hasChildren ? (
                    <button
                      type="button"
                      onClick={(e) => {
                        e.stopPropagation()
                        toggleExpand(span.span_id)
                      }}
                      className="w-4 h-4 flex items-center justify-center text-gray-400 hover:text-[#F4F4F4] flex-shrink-0"
                    >
                      <ChevronRight
                        className={`w-3 h-3 transition-transform ${isExpanded ? 'rotate-90' : ''}`}
                      />
                    </button>
                  ) : (
                    <div className="w-4 h-4 flex-shrink-0" />
                  )}

                  <div
                    className="w-2 h-2 rounded-sm flex-shrink-0"
                    style={{ backgroundColor: statusColors[span.status] }}
                  />

                  {span.service_name && (
                    <span className="flex-shrink-0 px-1.5 py-0.5 text-[10px] font-medium rounded border border-[#1D1D1D] bg-[#1D1D1D]/50 text-gray-300 leading-none">
                      {span.service_name}
                    </span>
                  )}
                  {span.kind && span.kind !== 'unspecified' && (
                    <span className="flex-shrink-0 px-1.5 py-0.5 text-[10px] font-medium rounded border border-[#1D1D1D] bg-[#141414] text-gray-400 leading-none capitalize">
                      {span.kind.toLowerCase()}
                    </span>
                  )}

                  <span
                    className={`text-[13px] font-medium truncate ${isSelected ? 'text-[#F3F724]' : 'text-[#F4F4F4]'}`}
                    title={span.name}
                  >
                    {span.name}
                  </span>

                  <span className="font-mono text-[11px] text-gray-400 flex-shrink-0 ml-auto">
                    {formatDuration(span.duration_ms)}
                  </span>
                </div>

                <div className="relative h-6 rounded bg-[linear-gradient(90deg,transparent_0%,transparent_25%,#1D1D1D_25%,#1D1D1D_25.1%,transparent_25.1%,transparent_50%,#1D1D1D_50%,#1D1D1D_50.1%,transparent_50.1%,transparent_75%,#1D1D1D_75%,#1D1D1D_75.1%,transparent_75.1%)]">
                  <div
                    className={`
                      absolute h-4 top-1 rounded-[3px] min-w-[3px] transition-all duration-150
                      ${isSelected ? 'scale-y-[1.3] shadow-[0_0_6px_rgba(243,247,36,0.4)]' : isHovered ? 'scale-y-[1.2] shadow-[0_0_0_2px_rgba(243,247,36,0.3)]' : ''}
                    `}
                    style={{
                      ...barStyle,
                      left: `${span.start_percent}%`,
                      width: `${Math.max(0.5, span.width_percent)}%`,
                    }}
                  />
                </div>
              </button>
            )
          })}
        </div>

        {contentHeight > (containerRef.current?.clientHeight || 0) && (
          <div className="w-16 border-l border-[#1D1D1D] bg-[#141414]/50 flex-shrink-0 relative p-2">
            <div className="text-[9px] text-gray-400 uppercase tracking-wider mb-2">Map</div>
            <div
              className="relative bg-[#1D1D1D] rounded overflow-hidden"
              style={{ height: miniMapHeight }}
            >
              {data.spans.map((span, i) => {
                const isError = span.status === 'error'
                const barColor = isError ? '#EF4444' : '#6B7280'

                return (
                  <div
                    key={span.span_id}
                    className="absolute h-[2px]"
                    style={{
                      backgroundColor: barColor,
                      opacity: isError ? 0.7 : 0.5,
                      top: `${(i / data.spans.length) * 100}%`,
                      left: `${span.start_percent}%`,
                      width: `${Math.max(2, span.width_percent)}%`,
                    }}
                  />
                )
              })}
              <div
                className="absolute left-0 right-0 bg-[#F3F724]/20 border border-[#F3F724]/40 rounded-sm"
                style={{
                  top: thumbPosition,
                  height: thumbHeight,
                }}
              />
            </div>
          </div>
        )}
      </div>
    </div>
  )
}
