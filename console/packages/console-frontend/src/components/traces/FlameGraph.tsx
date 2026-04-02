import { Minus, Plus, RotateCcw } from 'lucide-react'
import { useCallback, useEffect, useMemo, useReducer, useRef } from 'react'
import { getServiceColor, SPAN_STATUS_COLORS } from '@/lib/traceColors'
import type { VisualizationSpan, WaterfallData } from '@/lib/traceTransform'

interface FlameGraphProps {
  data: WaterfallData
  onSpanClick: (span: VisualizationSpan) => void
  selectedSpanId?: string
}

interface FlameNode {
  span: VisualizationSpan
  x: number
  width: number
  depth: number
  children: FlameNode[]
  selfTime: number
}

const STATUS_COLORS = {
  ok: { bg: SPAN_STATUS_COLORS.ok, hover: '#4ade80' },
  error: { bg: SPAN_STATUS_COLORS.error, hover: '#f87171' },
  unset: { bg: SPAN_STATUS_COLORS.unset, hover: '#9ca3af' },
}

interface ViewState {
  hoveredNode: FlameNode | null
  tooltipPos: { x: number; y: number }
  colorBy: 'status' | 'service'
  zoomLevel: number
  panOffset: number
}

type ViewAction =
  | { type: 'SET_HOVERED'; node: FlameNode | null; x: number; y: number }
  | { type: 'SET_COLOR_BY'; colorBy: 'status' | 'service' }
  | { type: 'SET_ZOOM'; zoomLevel: number }
  | { type: 'SET_PAN'; panOffset: number }
  | { type: 'RESET_ZOOM' }

const initialViewState: ViewState = {
  hoveredNode: null,
  tooltipPos: { x: 0, y: 0 },
  colorBy: 'status',
  zoomLevel: 1,
  panOffset: 0,
}

function viewReducer(state: ViewState, action: ViewAction): ViewState {
  switch (action.type) {
    case 'SET_HOVERED':
      return { ...state, hoveredNode: action.node, tooltipPos: { x: action.x, y: action.y } }
    case 'SET_COLOR_BY':
      return { ...state, colorBy: action.colorBy }
    case 'SET_ZOOM':
      return { ...state, zoomLevel: action.zoomLevel }
    case 'SET_PAN':
      return { ...state, panOffset: action.panOffset }
    case 'RESET_ZOOM':
      return { ...state, zoomLevel: 1, panOffset: 0 }
  }
}

function buildFlameNodes(spans: VisualizationSpan[]): FlameNode[] {
  const spanMap = new Map<string, FlameNode>()
  const roots: FlameNode[] = []

  for (const span of spans) {
    const node: FlameNode = {
      span,
      x: span.start_percent,
      width: span.width_percent,
      depth: span.depth,
      children: [],
      selfTime: span.duration_ms,
    }
    spanMap.set(span.span_id, node)
  }

  for (const span of spans) {
    const node = spanMap.get(span.span_id)
    if (!node) continue

    if (span.parent_span_id && spanMap.has(span.parent_span_id)) {
      const parent = spanMap.get(span.parent_span_id)
      if (parent) parent.children.push(node)
    } else {
      roots.push(node)
    }
  }

  // Calculate self-time (duration minus children's duration)
  for (const node of spanMap.values()) {
    let childrenDuration = 0
    for (const child of node.children) {
      childrenDuration += child.span.duration_ms
    }
    node.selfTime = Math.max(0, node.span.duration_ms - childrenDuration)
  }

  return roots
}

function flattenFlameNodes(nodes: FlameNode[]): FlameNode[] {
  const result: FlameNode[] = []

  function traverse(node: FlameNode) {
    result.push(node)
    for (const child of node.children) {
      traverse(child)
    }
  }

  for (const node of nodes) {
    traverse(node)
  }
  return result
}

function formatDuration(ms: number): string {
  if (ms < 0.001) return '0us'
  if (ms < 1) return `${(ms * 1000).toFixed(0)}us`
  if (ms < 1000) return `${ms.toFixed(2)}ms`
  return `${(ms / 1000).toFixed(2)}s`
}

export function FlameGraph({ data, onSpanClick, selectedSpanId }: FlameGraphProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const minimapRef = useRef<HTMLCanvasElement>(null)
  const containerRef = useRef<HTMLDivElement>(null)
  const [viewState, dispatch] = useReducer(viewReducer, initialViewState)
  const { hoveredNode, tooltipPos, colorBy, zoomLevel, panOffset } = viewState

  const ROW_HEIGHT = 26
  const ROW_GAP = 2
  const PADDING = 16
  const MINIMAP_HEIGHT = 32

  const flameNodes = useMemo(() => buildFlameNodes(data.spans), [data.spans])
  const flatNodes = useMemo(() => flattenFlameNodes(flameNodes), [flameNodes])
  const maxDepth = useMemo(() => Math.max(...data.spans.map((s) => s.depth), 0) + 1, [data.spans])

  const serviceColorMap = useMemo(() => {
    const services = new Set<string>()
    for (const span of data.spans) {
      const service = span.service_name || span.name.split('.')[0]
      services.add(service)
    }
    const map = new Map<string, string>()
    for (const service of services) {
      map.set(service, getServiceColor(service))
    }
    return map
  }, [data.spans])

  const serviceList = useMemo(() => Array.from(serviceColorMap.entries()), [serviceColorMap])

  const getNodeColor = useCallback(
    (node: FlameNode, isHovered: boolean, isSelected: boolean) => {
      if (colorBy === 'status') {
        const status = node.span.status
        const colors = STATUS_COLORS[status]
        if (isSelected) return '#F3F724'
        return isHovered ? colors.hover : colors.bg
      }
      const service = node.span.service_name || node.span.name.split('.')[0]
      const color = serviceColorMap.get(service) || '#6e7681'
      if (isSelected) return '#F3F724'
      return isHovered ? color : `${color}cc`
    },
    [colorBy, serviceColorMap],
  )

  const draw = useCallback(() => {
    const canvas = canvasRef.current
    const container = containerRef.current
    if (!canvas || !container) return

    const ctx = canvas.getContext('2d')
    if (!ctx) return

    const dpr = window.devicePixelRatio || 1
    const width = container.clientWidth
    const height = maxDepth * (ROW_HEIGHT + ROW_GAP) + PADDING * 2 + 20 // extra for time ruler

    canvas.width = width * dpr
    canvas.height = height * dpr
    canvas.style.width = `${width}px`
    canvas.style.height = `${height}px`
    ctx.scale(dpr, dpr)

    // Clear
    ctx.fillStyle = '#0A0A0A'
    ctx.fillRect(0, 0, width, height)

    // Draw grid lines (subtle)
    ctx.strokeStyle = '#161616'
    ctx.lineWidth = 1
    for (let i = 0; i <= 4; i++) {
      const x = PADDING + (width - PADDING * 2) * (i / 4)
      ctx.beginPath()
      ctx.moveTo(x, PADDING)
      ctx.lineTo(x, height - PADDING - 16)
      ctx.stroke()
    }

    // Draw spans
    const graphWidth = (width - PADDING * 2) * zoomLevel
    const offsetX = panOffset

    for (const node of flatNodes) {
      const x = PADDING + (node.x / 100) * graphWidth - offsetX
      const w = Math.max(2, (node.width / 100) * graphWidth)
      const y = PADDING + node.depth * (ROW_HEIGHT + ROW_GAP)

      // Skip if out of view
      if (x + w < 0 || x > width) continue

      const isHovered = hoveredNode?.span.span_id === node.span.span_id
      const isSelected = selectedSpanId === node.span.span_id

      // Depth-based opacity: deeper spans get slightly more transparent
      const depthAlpha = Math.max(0.7, 1 - node.depth * 0.05)

      // Draw bar with rounded corners
      const fillColor = getNodeColor(node, isHovered, isSelected)
      ctx.globalAlpha = isHovered || isSelected ? 1 : depthAlpha
      ctx.fillStyle = fillColor
      ctx.beginPath()
      ctx.roundRect(x, y, w, ROW_HEIGHT, 4)
      ctx.fill()

      // Draw border
      if (isSelected) {
        ctx.strokeStyle = '#F3F724'
        ctx.lineWidth = 2
        ctx.globalAlpha = 1
        ctx.stroke()
      } else if (isHovered) {
        ctx.strokeStyle = '#F3F724'
        ctx.lineWidth = 1.5
        ctx.globalAlpha = 0.8
        ctx.stroke()
      }

      ctx.globalAlpha = 1

      // Draw text if wide enough
      if (w > 50) {
        const textColor = isSelected ? '#0A0A0A' : '#F4F4F4'
        ctx.fillStyle = textColor
        ctx.font = '11px JetBrains Mono, monospace'
        ctx.textBaseline = 'middle'

        const spanName = node.span.name
        const duration = formatDuration(node.span.duration_ms)
        const maxTextWidth = w - 12

        // Try to fit name + duration
        const fullText = `${spanName}  ${duration}`
        let displayText = fullText

        if (ctx.measureText(fullText).width > maxTextWidth) {
          // Fall back to just the name
          displayText = spanName
          while (ctx.measureText(displayText).width > maxTextWidth && displayText.length > 3) {
            displayText = `${displayText.slice(0, -4)}...`
          }
        }

        if (displayText.length > 3) {
          ctx.fillText(displayText, x + 6, y + ROW_HEIGHT / 2)

          // If we showed the full text, dim the duration part
          if (displayText === fullText && w > 120) {
            const nameWidth = ctx.measureText(`${spanName}  `).width
            ctx.fillStyle = isSelected ? 'rgba(10,10,10,0.6)' : 'rgba(156,163,175,0.8)'
            ctx.fillText(duration, x + 6 + nameWidth, y + ROW_HEIGHT / 2)
          }
        }
      } else if (w > 20) {
        // Show abbreviated duration for medium-sized bars
        ctx.fillStyle = isSelected ? '#0A0A0A' : 'rgba(244,244,244,0.7)'
        ctx.font = '9px JetBrains Mono, monospace'
        ctx.textBaseline = 'middle'
        const dur = formatDuration(node.span.duration_ms)
        if (ctx.measureText(dur).width < w - 6) {
          ctx.fillText(dur, x + 3, y + ROW_HEIGHT / 2)
        }
      }
    }

    // Draw time ruler at bottom
    ctx.fillStyle = '#6B7280'
    ctx.font = '10px JetBrains Mono, monospace'
    ctx.textBaseline = 'top'
    for (let i = 0; i <= 4; i++) {
      const x = PADDING + (width - PADDING * 2) * (i / 4)
      const time = formatDuration(
        (data.total_duration_ms / zoomLevel) * (i / 4) +
          (panOffset / graphWidth) * data.total_duration_ms,
      )
      const textWidth = ctx.measureText(time).width
      const textX = i === 4 ? x - textWidth : i === 0 ? x : x - textWidth / 2
      ctx.fillText(time, textX, height - PADDING + 2)
    }
  }, [
    flatNodes,
    maxDepth,
    hoveredNode,
    selectedSpanId,
    getNodeColor,
    zoomLevel,
    panOffset,
    data.total_duration_ms,
  ])

  // Draw minimap
  const drawMinimap = useCallback(() => {
    const minimap = minimapRef.current
    const container = containerRef.current
    if (!minimap || !container || zoomLevel <= 1) return

    const ctx = minimap.getContext('2d')
    if (!ctx) return

    const dpr = window.devicePixelRatio || 1
    const width = container.clientWidth
    const height = MINIMAP_HEIGHT

    minimap.width = width * dpr
    minimap.height = height * dpr
    minimap.style.width = `${width}px`
    minimap.style.height = `${height}px`
    ctx.scale(dpr, dpr)

    // Background
    ctx.fillStyle = '#0D0D0D'
    ctx.fillRect(0, 0, width, height)

    // Draw all spans as thin bars
    const barHeight = Math.max(1, Math.min(3, height / (maxDepth + 1)))
    const topPad = (height - maxDepth * barHeight) / 2

    for (const node of flatNodes) {
      const x = PADDING + (node.x / 100) * (width - PADDING * 2)
      const w = Math.max(1, (node.width / 100) * (width - PADDING * 2))
      const y = topPad + node.depth * barHeight

      const service = node.span.service_name || node.span.name.split('.')[0]
      ctx.fillStyle =
        colorBy === 'status'
          ? STATUS_COLORS[node.span.status]?.bg || '#6E7681'
          : serviceColorMap.get(service) || '#6e7681'
      ctx.globalAlpha = 0.6
      ctx.fillRect(x, y, w, Math.max(1, barHeight - 0.5))
    }

    ctx.globalAlpha = 1

    // Draw viewport indicator
    const graphWidth = (width - PADDING * 2) * zoomLevel
    const viewStart = (panOffset / graphWidth) * (width - PADDING * 2)
    const viewWidth = (width - PADDING * 2) / zoomLevel

    ctx.strokeStyle = '#F3F724'
    ctx.lineWidth = 1
    ctx.globalAlpha = 0.6
    ctx.strokeRect(PADDING + viewStart, 1, viewWidth, height - 2)

    // Fill viewport area with subtle highlight
    ctx.fillStyle = 'rgba(243, 247, 36, 0.04)'
    ctx.fillRect(PADDING + viewStart, 1, viewWidth, height - 2)

    ctx.globalAlpha = 1
  }, [flatNodes, maxDepth, zoomLevel, panOffset, colorBy, serviceColorMap])

  useEffect(() => {
    draw()
    drawMinimap()

    const handleResize = () => {
      draw()
      drawMinimap()
    }
    window.addEventListener('resize', handleResize)
    return () => window.removeEventListener('resize', handleResize)
  }, [draw, drawMinimap])

  const handleMouseMove = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const canvas = canvasRef.current
    const container = containerRef.current
    if (!canvas || !container) return

    const rect = canvas.getBoundingClientRect()
    const x = e.clientX - rect.left
    const y = e.clientY - rect.top
    const width = container.clientWidth
    const graphWidth = (width - PADDING * 2) * zoomLevel

    let found: FlameNode | null = null
    for (const node of flatNodes) {
      const nodeX = PADDING + (node.x / 100) * graphWidth - panOffset
      const nodeW = Math.max(2, (node.width / 100) * graphWidth)
      const nodeY = PADDING + node.depth * (ROW_HEIGHT + ROW_GAP)

      if (x >= nodeX && x <= nodeX + nodeW && y >= nodeY && y <= nodeY + ROW_HEIGHT) {
        found = node
        break
      }
    }

    dispatch({ type: 'SET_HOVERED', node: found, x: e.clientX, y: e.clientY })
  }

  const handleMouseLeave = () => {
    dispatch({ type: 'SET_HOVERED', node: null, x: 0, y: 0 })
  }

  const handleClick = () => {
    if (hoveredNode) {
      onSpanClick(hoveredNode.span)
    }
  }

  const handleWheel = (e: React.WheelEvent<HTMLCanvasElement>) => {
    if (e.ctrlKey || e.metaKey) {
      e.preventDefault()
      const delta = e.deltaY > 0 ? 0.9 : 1.1
      dispatch({ type: 'SET_ZOOM', zoomLevel: Math.max(1, Math.min(20, zoomLevel * delta)) })
    } else {
      dispatch({ type: 'SET_PAN', panOffset: Math.max(0, panOffset + e.deltaY) })
    }
  }

  const handleMinimapClick = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const minimap = minimapRef.current
    const container = containerRef.current
    if (!minimap || !container) return

    const rect = minimap.getBoundingClientRect()
    const clickX = e.clientX - rect.left
    const width = container.clientWidth
    const graphWidth = (width - PADDING * 2) * zoomLevel

    // Center viewport on click position
    const clickPercent = (clickX - PADDING) / (width - PADDING * 2)
    const newOffset = clickPercent * graphWidth - (width - PADDING * 2) / 2
    dispatch({ type: 'SET_PAN', panOffset: Math.max(0, newOffset) })
  }

  const handleReset = () => {
    dispatch({ type: 'RESET_ZOOM' })
  }

  const zoomIn = () => dispatch({ type: 'SET_ZOOM', zoomLevel: Math.min(20, zoomLevel * 1.3) })
  const zoomOut = () => dispatch({ type: 'SET_ZOOM', zoomLevel: Math.max(1, zoomLevel / 1.3) })

  const tracePercent = hoveredNode
    ? ((hoveredNode.span.duration_ms / data.total_duration_ms) * 100).toFixed(1)
    : '0'

  return (
    <div className="flex flex-col h-full">
      {/* Compact toolbar */}
      <div className="flex items-center justify-between px-4 py-2 border-b border-[#1D1D1D] bg-[#0A0A0A] flex-shrink-0">
        <div className="flex items-center gap-1.5">
          <span className="text-[10px] text-gray-500 uppercase tracking-wider mr-1">Color</span>
          <button
            type="button"
            onClick={() => dispatch({ type: 'SET_COLOR_BY', colorBy: 'status' })}
            className={`px-2 py-0.5 text-[10px] rounded transition-colors ${
              colorBy === 'status'
                ? 'bg-[#F3F724] text-black font-semibold'
                : 'bg-[#141414] text-gray-500 hover:text-gray-300 border border-[#1D1D1D]'
            }`}
          >
            Status
          </button>
          <button
            type="button"
            onClick={() => dispatch({ type: 'SET_COLOR_BY', colorBy: 'service' })}
            className={`px-2 py-0.5 text-[10px] rounded transition-colors ${
              colorBy === 'service'
                ? 'bg-[#F3F724] text-black font-semibold'
                : 'bg-[#141414] text-gray-500 hover:text-gray-300 border border-[#1D1D1D]'
            }`}
          >
            Service
          </button>

          {/* Inline legend */}
          <div className="w-px h-3.5 bg-[#1D1D1D] mx-1.5" />
          {colorBy === 'status' ? (
            <div className="flex items-center gap-2.5">
              <div className="flex items-center gap-1">
                <div
                  className="w-2 h-2 rounded-sm"
                  style={{ backgroundColor: STATUS_COLORS.ok.bg }}
                />
                <span className="text-[9px] text-gray-500">OK</span>
              </div>
              <div className="flex items-center gap-1">
                <div
                  className="w-2 h-2 rounded-sm"
                  style={{ backgroundColor: STATUS_COLORS.error.bg }}
                />
                <span className="text-[9px] text-gray-500">Error</span>
              </div>
              <div className="flex items-center gap-1">
                <div
                  className="w-2 h-2 rounded-sm"
                  style={{ backgroundColor: STATUS_COLORS.unset.bg }}
                />
                <span className="text-[9px] text-gray-500">Unset</span>
              </div>
            </div>
          ) : (
            <div className="flex items-center gap-2 overflow-hidden max-w-[200px]">
              {serviceList.map(([name, color]) => (
                <div key={name} className="flex items-center gap-1 flex-shrink-0">
                  <div className="w-2 h-2 rounded-sm" style={{ backgroundColor: color }} />
                  <span className="text-[9px] text-gray-500 truncate max-w-[60px]">{name}</span>
                </div>
              ))}
            </div>
          )}
        </div>

        {/* Zoom controls */}
        <div className="flex items-center gap-1">
          {zoomLevel > 1 && (
            <span className="text-[10px] font-mono text-gray-500 mr-1">
              {Math.round(zoomLevel * 100)}%
            </span>
          )}
          <button
            type="button"
            onClick={zoomOut}
            disabled={zoomLevel <= 1}
            className="p-1 rounded hover:bg-[#141414] text-gray-500 hover:text-gray-300 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
            title="Zoom out"
          >
            <Minus className="w-3 h-3" />
          </button>
          <button
            type="button"
            onClick={zoomIn}
            disabled={zoomLevel >= 20}
            className="p-1 rounded hover:bg-[#141414] text-gray-500 hover:text-gray-300 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
            title="Zoom in"
          >
            <Plus className="w-3 h-3" />
          </button>
          <button
            type="button"
            onClick={handleReset}
            disabled={zoomLevel === 1 && panOffset === 0}
            className="p-1 rounded hover:bg-[#141414] text-gray-500 hover:text-gray-300 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
            title="Reset view"
          >
            <RotateCcw className="w-3 h-3" />
          </button>
        </div>
      </div>

      {/* Minimap (only when zoomed) */}
      {zoomLevel > 1 && (
        <div className="border-b border-[#1D1D1D] flex-shrink-0 px-0">
          <canvas
            ref={minimapRef}
            onClick={handleMinimapClick}
            className="cursor-pointer w-full"
            style={{ height: MINIMAP_HEIGHT }}
          />
        </div>
      )}

      {/* Canvas */}
      <div ref={containerRef} className="relative flex-1 overflow-hidden">
        <canvas
          ref={canvasRef}
          onMouseMove={handleMouseMove}
          onMouseLeave={handleMouseLeave}
          onClick={handleClick}
          onWheel={handleWheel}
          className="cursor-pointer"
        />
      </div>

      {/* Enhanced Tooltip */}
      {hoveredNode && (
        <div
          className="fixed z-50 pointer-events-none"
          style={{
            left: Math.min(tooltipPos.x + 12, window.innerWidth - 280),
            top: tooltipPos.y + 12,
          }}
        >
          <div className="bg-[#141414] border border-[#2D2D2D] rounded-lg px-3 py-2.5 shadow-xl shadow-black/50 min-w-[200px] max-w-[280px]">
            {/* Span name */}
            <div className="font-semibold text-[12px] text-white leading-tight mb-1.5 break-all">
              {hoveredNode.span.name}
            </div>

            {/* Service badge */}
            <div className="flex items-center gap-1.5 mb-2">
              <div
                className="w-1.5 h-1.5 rounded-full flex-shrink-0"
                style={{
                  backgroundColor:
                    serviceColorMap.get(
                      hoveredNode.span.service_name || hoveredNode.span.name.split('.')[0],
                    ) || '#6e7681',
                }}
              />
              <span className="text-[10px] text-gray-400 font-mono">
                {hoveredNode.span.service_name || hoveredNode.span.name.split('.')[0]}
              </span>
            </div>

            {/* Stats grid */}
            <div className="grid grid-cols-2 gap-x-4 gap-y-1 text-[10px]">
              <div className="flex justify-between">
                <span className="text-gray-500">Duration</span>
                <span className="text-[#F3F724] font-mono font-semibold">
                  {formatDuration(hoveredNode.span.duration_ms)}
                </span>
              </div>
              <div className="flex justify-between">
                <span className="text-gray-500">Self</span>
                <span className="text-gray-300 font-mono">
                  {formatDuration(hoveredNode.selfTime)}
                </span>
              </div>
              <div className="flex justify-between">
                <span className="text-gray-500">% Trace</span>
                <span className="text-gray-300 font-mono">{tracePercent}%</span>
              </div>
              <div className="flex justify-between">
                <span className="text-gray-500">Status</span>
                <span
                  className="font-mono font-medium"
                  style={{ color: STATUS_COLORS[hoveredNode.span.status]?.bg || '#6E7681' }}
                >
                  {hoveredNode.span.status}
                </span>
              </div>
            </div>

            {/* Trace percentage bar */}
            <div className="mt-2 h-1 rounded-full bg-[#1D1D1D] overflow-hidden">
              <div
                className="h-full rounded-full bg-[#F3F724] transition-all duration-150"
                style={{ width: `${Math.max(1, Number.parseFloat(tracePercent))}%`, opacity: 0.7 }}
              />
            </div>

            {/* Children info */}
            {hoveredNode.children.length > 0 && (
              <div className="mt-1.5 text-[9px] text-gray-600">
                {hoveredNode.children.length} child span
                {hoveredNode.children.length !== 1 ? 's' : ''}
              </div>
            )}
          </div>
        </div>
      )}

      {/* Keyboard hint */}
      {zoomLevel <= 1 && (
        <div className="flex-shrink-0 px-4 py-1.5 text-[9px] text-gray-600 text-center border-t border-[#1D1D1D]">
          Ctrl+Scroll to zoom | Scroll to pan | Click to select
        </div>
      )}
    </div>
  )
}
