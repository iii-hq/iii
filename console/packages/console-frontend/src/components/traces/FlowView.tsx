import { Maximize2, Minus, Plus } from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { getServiceColor } from '@/lib/traceColors'
import type { VisualizationSpan, WaterfallData } from '@/lib/traceTransform'
import { classifySpanType, formatDuration, getServiceName } from '@/lib/traceUtils'

interface FlowViewProps {
  data: WaterfallData
  onSpanClick: (span: VisualizationSpan) => void
  selectedSpanId?: string
}

interface LayoutNode {
  span: VisualizationSpan
  x: number
  y: number
  children: LayoutNode[]
  subtreeWidth: number
}

const NODE_W = 240
const NODE_H = 72
const GAP_X = 32
const GAP_Y = 64
const CONNECTOR_DASH = '6 4'
const MINIMAP_W = 140
const MINIMAP_H = 100

const SPAN_TYPE_STYLES: Record<string, string> = {
  trigger: 'text-green-400 bg-green-500/10',
  enqueue: 'text-[#F3F724] bg-[#F3F724]/10',
  function: 'text-cyan-400 bg-cyan-500/10',
}

function getNodeBorderClass(isSelected: boolean, isError: boolean): string {
  if (isSelected) {
    return 'border-[#F3F724] shadow-[0_0_12px_rgba(243,247,36,0.2)] ring-1 ring-[#F3F724]/30'
  }
  if (isError) {
    return 'border-[#EF4444]/40 hover:border-[#EF4444]/70'
  }
  return 'border-[#1D1D1D] hover:border-[#3D3D3D]'
}

function buildTree(spans: VisualizationSpan[]): LayoutNode[] {
  const childrenMap = new Map<string, VisualizationSpan[]>()
  const roots: VisualizationSpan[] = []
  const spanById = new Map<string, VisualizationSpan>()

  for (const span of spans) {
    spanById.set(span.span_id, span)
  }

  for (const span of spans) {
    if (!span.parent_span_id || !spanById.has(span.parent_span_id)) {
      roots.push(span)
    } else {
      const list = childrenMap.get(span.parent_span_id) || []
      list.push(span)
      childrenMap.set(span.parent_span_id, list)
    }
  }

  function toNode(span: VisualizationSpan): LayoutNode {
    const kids = (childrenMap.get(span.span_id) || [])
      .sort((a, b) => a.start_percent - b.start_percent)
      .map(toNode)

    const childrenTotalWidth =
      kids.length > 0
        ? kids.reduce((sum, k) => sum + k.subtreeWidth, 0) + (kids.length - 1) * GAP_X
        : 0

    return {
      span,
      x: 0,
      y: 0,
      children: kids,
      subtreeWidth: Math.max(NODE_W, childrenTotalWidth),
    }
  }

  const rootNodes = roots.sort((a, b) => a.start_percent - b.start_percent).map(toNode)

  return rootNodes
}

function layoutTree(roots: LayoutNode[]): { nodes: LayoutNode[]; width: number; height: number } {
  const allNodes: LayoutNode[] = []

  function assignPositions(node: LayoutNode, offsetX: number, depth: number) {
    node.y = depth * (NODE_H + GAP_Y)
    node.x = offsetX + (node.subtreeWidth - NODE_W) / 2
    allNodes.push(node)

    let childOffsetX = offsetX
    for (const child of node.children) {
      assignPositions(child, childOffsetX, depth + 1)
      childOffsetX += child.subtreeWidth + GAP_X
    }
  }

  const totalRootWidth =
    roots.reduce((sum, r) => sum + r.subtreeWidth, 0) + Math.max(0, roots.length - 1) * GAP_X
  let offsetX = 0
  for (const root of roots) {
    assignPositions(root, offsetX, 0)
    offsetX += root.subtreeWidth + GAP_X
  }

  let maxX = 0
  let maxY = 0
  for (const n of allNodes) {
    maxX = Math.max(maxX, n.x + NODE_W)
    maxY = Math.max(maxY, n.y + NODE_H)
  }

  return { nodes: allNodes, width: Math.max(totalRootWidth, maxX), height: maxY }
}

export function FlowView({ data, onSpanClick, selectedSpanId }: FlowViewProps) {
  const containerRef = useRef<HTMLDivElement>(null)
  const [zoom, setZoom] = useState(1)
  const [pan, setPan] = useState({ x: 0, y: 0 })
  const [isDragging, setIsDragging] = useState(false)
  const dragStartRef = useRef({ x: 0, y: 0, panX: 0, panY: 0 })
  const panRef = useRef(pan)
  panRef.current = pan

  const {
    nodes: layoutNodes,
    roots,
    totalWidth,
    totalHeight,
  } = useMemo(() => {
    const roots = buildTree(data.spans)
    const { nodes, width, height } = layoutTree(roots)
    return { nodes, roots, totalWidth: width, totalHeight: height }
  }, [data.spans])

  const edges = useMemo(() => {
    const result: Array<{
      fromX: number
      fromY: number
      toX: number
      toY: number
      parentId: string
      childId: string
    }> = []

    function walk(node: LayoutNode) {
      for (const child of node.children) {
        result.push({
          fromX: node.x + NODE_W / 2,
          fromY: node.y + NODE_H,
          toX: child.x + NODE_W / 2,
          toY: child.y,
          parentId: node.span.span_id,
          childId: child.span.span_id,
        })
        walk(child)
      }
    }
    for (const r of roots) walk(r)
    return result
  }, [roots])

  const colorByService = useMemo(() => {
    const map = new Map<string, string>()
    for (const span of data.spans) {
      const svc = getServiceName(span)
      if (!map.has(svc)) map.set(svc, getServiceColor(svc))
    }
    return map
  }, [data.spans])

  const services = useMemo(() => Array.from(colorByService.entries()), [colorByService])

  const fitView = useCallback(() => {
    const container = containerRef.current
    if (!container || totalWidth === 0) return
    const padding = 60
    const cw = container.clientWidth - padding * 2
    const ch = container.clientHeight - padding * 2
    const scaleX = cw / totalWidth
    const scaleY = ch / totalHeight
    const newZoom = Math.min(scaleX, scaleY, 1.2)
    const newPanX = (container.clientWidth - totalWidth * newZoom) / 2
    const newPanY = (container.clientHeight - totalHeight * newZoom) / 2
    setZoom(newZoom)
    setPan({ x: newPanX, y: Math.max(20, newPanY) })
  }, [totalWidth, totalHeight])

  useEffect(() => {
    fitView()
  }, [fitView])

  const handleWheel = useCallback((e: React.WheelEvent) => {
    e.preventDefault()
    const delta = e.deltaY > 0 ? 0.9 : 1.1
    setZoom((z) => Math.max(0.1, Math.min(3, z * delta)))
  }, [])

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    if (e.button !== 0) return
    setIsDragging(true)
    dragStartRef.current = {
      x: e.clientX,
      y: e.clientY,
      panX: panRef.current.x,
      panY: panRef.current.y,
    }
  }, [])

  const handleMouseMove = useCallback(
    (e: React.MouseEvent) => {
      if (!isDragging) return
      const dx = e.clientX - dragStartRef.current.x
      const dy = e.clientY - dragStartRef.current.y
      setPan({ x: dragStartRef.current.panX + dx, y: dragStartRef.current.panY + dy })
    },
    [isDragging],
  )

  const handleMouseUp = useCallback(() => {
    setIsDragging(false)
  }, [])

  const minimapScale = useMemo(() => {
    if (totalWidth === 0 || totalHeight === 0) return 1
    return Math.min(MINIMAP_W / totalWidth, MINIMAP_H / totalHeight) * 0.85
  }, [totalWidth, totalHeight])

  return (
    <div className="relative w-full h-full overflow-hidden bg-[#080808]" ref={containerRef}>
      {/* Header row */}
      <div className="absolute top-0 left-0 right-0 z-10 flex items-center justify-between px-4 py-2.5 pointer-events-none">
        <span className="text-[10px] text-gray-500 uppercase tracking-wider font-medium pointer-events-auto">
          Flow View
        </span>
        <button
          type="button"
          onClick={fitView}
          className="flex items-center gap-1.5 text-[10px] text-gray-500 hover:text-gray-300 transition-colors pointer-events-auto"
        >
          <Maximize2 className="w-3 h-3" />
          Fit View
        </button>
      </div>

      {/* Pannable / zoomable canvas */}
      {/* biome-ignore lint/a11y/noStaticElementInteractions: interactive canvas for pan/zoom */}
      <div
        className={`w-full h-full ${isDragging ? 'cursor-grabbing' : 'cursor-grab'}`}
        onWheel={handleWheel}
        onMouseDown={handleMouseDown}
        onMouseMove={handleMouseMove}
        onMouseUp={handleMouseUp}
        onMouseLeave={handleMouseUp}
      >
        <div
          style={{
            transform: `translate(${pan.x}px, ${pan.y}px) scale(${zoom})`,
            transformOrigin: '0 0',
            width: totalWidth,
            height: totalHeight,
            position: 'relative',
          }}
        >
          {/* SVG edges */}
          <svg
            className="absolute inset-0 pointer-events-none"
            width={totalWidth}
            height={totalHeight}
            style={{ overflow: 'visible' }}
            role="img"
            aria-label="Flow edges"
          >
            {edges.map((e) => {
              const midY = (e.fromY + e.toY) / 2
              return (
                <path
                  key={`${e.parentId}-${e.childId}`}
                  d={`M ${e.fromX} ${e.fromY} C ${e.fromX} ${midY}, ${e.toX} ${midY}, ${e.toX} ${e.toY}`}
                  fill="none"
                  stroke="#333"
                  strokeWidth={1.5}
                  strokeDasharray={CONNECTOR_DASH}
                  opacity={0.6}
                />
              )
            })}
          </svg>

          {/* Nodes */}
          {layoutNodes.map((node) => {
            const span = node.span
            const svc = getServiceName(span)
            const color = colorByService.get(svc) ?? '#666666'
            const type = classifySpanType(span)
            const isSelected = selectedSpanId === span.span_id
            const isError = span.status === 'error'

            return (
              <button
                key={span.span_id}
                type="button"
                onClick={(e) => {
                  e.stopPropagation()
                  onSpanClick(span)
                }}
                className={`absolute rounded-lg border transition-all duration-150 text-left group cursor-pointer ${getNodeBorderClass(isSelected, isError)} bg-[#0E0E0E] hover:bg-[#141414]`}
                style={{
                  left: node.x,
                  top: node.y,
                  width: NODE_W,
                  height: NODE_H,
                }}
              >
                {/* Service color bar */}
                <div
                  className="absolute top-0 left-0 right-0 h-[3px] rounded-t-lg"
                  style={{ backgroundColor: color }}
                />

                <div className="flex flex-col justify-between h-full px-3 pt-2.5 pb-2">
                  <div className="flex items-start justify-between gap-2">
                    <div className="flex items-center gap-1.5 min-w-0 flex-1">
                      <span
                        className="inline-block px-1.5 py-0.5 rounded text-[8px] font-bold uppercase tracking-wider flex-shrink-0"
                        style={{
                          backgroundColor: `${color}18`,
                          color: color,
                          border: `1px solid ${color}30`,
                        }}
                      >
                        {svc}
                      </span>
                    </div>
                    <span
                      className={`text-[8px] font-mono uppercase tracking-wider flex-shrink-0 px-1 py-0.5 rounded ${SPAN_TYPE_STYLES[type] ?? SPAN_TYPE_STYLES.function}`}
                    >
                      {type}
                    </span>
                  </div>

                  <div className="min-w-0">
                    <div
                      className="text-[11px] font-mono text-gray-200 truncate group-hover:text-white transition-colors"
                      title={span.name}
                    >
                      {span.name}
                    </div>
                    <div className="text-[9px] font-mono text-gray-500">
                      {formatDuration(span.duration_ms)}
                    </div>
                  </div>
                </div>

                {/* Error indicator */}
                {isError && (
                  <div className="absolute -top-1 -right-1 w-2.5 h-2.5 rounded-full bg-[#EF4444] border border-[#0E0E0E]" />
                )}
              </button>
            )
          })}
        </div>
      </div>

      {/* Zoom controls - bottom left */}
      <div className="absolute bottom-10 left-4 z-10 flex flex-col gap-1">
        <button
          type="button"
          onClick={() => setZoom((z) => Math.min(3, z * 1.2))}
          className="w-7 h-7 flex items-center justify-center rounded bg-[#141414] border border-[#1D1D1D] text-gray-400 hover:text-white hover:border-[#3D3D3D] transition-colors"
        >
          <Plus className="w-3.5 h-3.5" />
        </button>
        <button
          type="button"
          onClick={() => setZoom((z) => Math.max(0.1, z * 0.8))}
          className="w-7 h-7 flex items-center justify-center rounded bg-[#141414] border border-[#1D1D1D] text-gray-400 hover:text-white hover:border-[#3D3D3D] transition-colors"
        >
          <Minus className="w-3.5 h-3.5" />
        </button>
        <button
          type="button"
          onClick={fitView}
          className="w-7 h-7 flex items-center justify-center rounded bg-[#141414] border border-[#1D1D1D] text-gray-400 hover:text-white hover:border-[#3D3D3D] transition-colors"
        >
          <Maximize2 className="w-3 h-3" />
        </button>
      </div>

      {/* Minimap - bottom right */}
      <div
        className="absolute bottom-10 right-4 z-10 rounded border border-[#1D1D1D] bg-[#0A0A0A]/90 overflow-hidden"
        style={{ width: MINIMAP_W, height: MINIMAP_H }}
      >
        <svg width={MINIMAP_W} height={MINIMAP_H} role="img" aria-label="Flow minimap">
          <g
            transform={`translate(${(MINIMAP_W - totalWidth * minimapScale) / 2}, ${(MINIMAP_H - totalHeight * minimapScale) / 2}) scale(${minimapScale})`}
          >
            {edges.map((e) => (
              <line
                key={`mm-${e.parentId}-${e.childId}`}
                x1={e.fromX}
                y1={e.fromY}
                x2={e.toX}
                y2={e.toY}
                stroke="#333"
                strokeWidth={2 / minimapScale}
              />
            ))}
            {layoutNodes.map((node) => {
              const color = colorByService.get(getServiceName(node.span)) ?? '#666'
              return (
                <rect
                  key={`mm-${node.span.span_id}`}
                  x={node.x}
                  y={node.y}
                  width={NODE_W}
                  height={NODE_H}
                  fill={color}
                  opacity={selectedSpanId === node.span.span_id ? 1 : 0.5}
                  rx={4 / minimapScale}
                />
              )
            })}
          </g>
        </svg>
      </div>

      {/* Legend - bottom center */}
      <div className="absolute bottom-2 left-1/2 -translate-x-1/2 z-10 flex items-center gap-4 px-3 py-1.5 rounded bg-[#0A0A0A]/80 border border-[#1D1D1D]">
        <span className="text-[9px] text-gray-600 mr-1">Click to select</span>
        <span className="text-[9px] text-gray-600">Scroll to zoom</span>
        <span className="text-[9px] text-gray-600">Drag to pan</span>
        <div className="w-px h-3 bg-[#1D1D1D]" />
        {services.map(([name, color]) => (
          <div key={name} className="flex items-center gap-1">
            <div className="w-2 h-2 rounded-sm flex-shrink-0" style={{ backgroundColor: color }} />
            <span className="text-[9px] font-mono text-gray-500">{name}</span>
          </div>
        ))}
        <div className="w-px h-3 bg-[#1D1D1D]" />
        <div className="flex items-center gap-1">
          <div className="w-2 h-2 rounded-full bg-[#EF4444] flex-shrink-0" />
          <span className="text-[9px] font-mono text-gray-500">Error</span>
        </div>
      </div>
    </div>
  )
}
