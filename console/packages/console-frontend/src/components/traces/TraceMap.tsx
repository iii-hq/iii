import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { getServiceColor } from '@/lib/traceColors'
import type { VisualizationSpan, WaterfallData } from '@/lib/traceTransform'

interface TraceMapProps {
  data: WaterfallData
  onSpanClick: (span: VisualizationSpan) => void
}

interface ServiceNode {
  id: string
  name: string
  spanCount: number
  totalDuration: number
  errorCount: number
  x: number
  y: number
  color: string
}

interface ServiceEdge {
  from: string
  to: string
  callCount: number
  totalDuration: number
}

function formatDuration(ms: number): string {
  if (ms < 1) return `${(ms * 1000).toFixed(0)}µs`
  if (ms < 1000) return `${ms.toFixed(2)}ms`
  return `${(ms / 1000).toFixed(2)}s`
}

export function TraceMap({ data, onSpanClick }: TraceMapProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const containerRef = useRef<HTMLDivElement>(null)
  const [hoveredNode, setHoveredNode] = useState<ServiceNode | null>(null)
  const [selectedNode, setSelectedNode] = useState<ServiceNode | null>(null)
  const [tooltipPos, setTooltipPos] = useState({ x: 0, y: 0 })

  // Build service graph
  const { nodes, edges } = useMemo(() => {
    const serviceMap = new Map<
      string,
      { spanCount: number; totalDuration: number; errorCount: number; children: Set<string> }
    >()
    const edgeMap = new Map<string, { callCount: number; totalDuration: number }>()

    // First pass: collect service info
    data.spans.forEach((span) => {
      const service = span.service_name || span.name.split('.')[0] || 'unknown'

      const existing = serviceMap.get(service) || {
        spanCount: 0,
        totalDuration: 0,
        errorCount: 0,
        children: new Set<string>(),
      }

      serviceMap.set(service, {
        spanCount: existing.spanCount + 1,
        totalDuration: existing.totalDuration + span.duration_ms,
        errorCount: existing.errorCount + (span.status === 'error' ? 1 : 0),
        children: existing.children,
      })
    })

    // Build span lookup for O(1) parent resolution
    const spanById = new Map(data.spans.map((s) => [s.span_id, s]))

    // Second pass: build edges based on parent-child relationships
    data.spans.forEach((span) => {
      if (span.parent_span_id) {
        const parentSpan = spanById.get(span.parent_span_id)
        if (parentSpan) {
          const fromService = parentSpan.service_name || parentSpan.name.split('.')[0] || 'unknown'
          const toService = span.service_name || span.name.split('.')[0] || 'unknown'

          if (fromService !== toService) {
            const edgeKey = `${fromService}->${toService}`
            const existing = edgeMap.get(edgeKey) || { callCount: 0, totalDuration: 0 }
            edgeMap.set(edgeKey, {
              callCount: existing.callCount + 1,
              totalDuration: existing.totalDuration + span.duration_ms,
            })

            serviceMap.get(fromService)?.children.add(toService)
          }
        }
      }
    })

    // Layout nodes in a circular pattern
    const serviceNames = Array.from(serviceMap.keys())
    const nodeCount = serviceNames.length
    const centerX = 400
    const centerY = 250
    const radius = Math.min(150, 50 + nodeCount * 20)

    const nodes: ServiceNode[] = serviceNames
      .map((name, i) => {
        const stats = serviceMap.get(name)
        if (!stats) return null
        const angle = (i / nodeCount) * 2 * Math.PI - Math.PI / 2

        return {
          id: name,
          name,
          spanCount: stats.spanCount,
          totalDuration: stats.totalDuration,
          errorCount: stats.errorCount,
          x: centerX + radius * Math.cos(angle),
          y: centerY + radius * Math.sin(angle),
          color: getServiceColor(name),
        }
      })
      .filter((n): n is ServiceNode => n !== null)

    const edges: ServiceEdge[] = Array.from(edgeMap.entries()).map(([key, value]) => {
      const [from, to] = key.split('->')
      return { from, to, ...value }
    })

    return { nodes, edges }
  }, [data.spans])

  const draw = useCallback(() => {
    const canvas = canvasRef.current
    const container = containerRef.current
    if (!canvas || !container) return

    const ctx = canvas.getContext('2d')
    if (!ctx) return

    const dpr = window.devicePixelRatio || 1
    const width = container.clientWidth
    const height = container.clientHeight

    canvas.width = width * dpr
    canvas.height = height * dpr
    canvas.style.width = `${width}px`
    canvas.style.height = `${height}px`
    ctx.scale(dpr, dpr)

    // Clear with black background
    ctx.fillStyle = '#0A0A0A'
    ctx.fillRect(0, 0, width, height)

    // Draw grid pattern
    ctx.strokeStyle = '#1D1D1D'
    ctx.lineWidth = 0.5
    for (let x = 0; x < width; x += 40) {
      ctx.beginPath()
      ctx.moveTo(x, 0)
      ctx.lineTo(x, height)
      ctx.stroke()
    }
    for (let y = 0; y < height; y += 40) {
      ctx.beginPath()
      ctx.moveTo(0, y)
      ctx.lineTo(width, y)
      ctx.stroke()
    }

    // Draw edges
    edges.forEach((edge) => {
      const fromNode = nodes.find((n) => n.id === edge.from)
      const toNode = nodes.find((n) => n.id === edge.to)
      if (!fromNode || !toNode) return

      const isHighlighted =
        hoveredNode?.id === edge.from ||
        hoveredNode?.id === edge.to ||
        selectedNode?.id === edge.from ||
        selectedNode?.id === edge.to

      // Draw line
      ctx.strokeStyle = isHighlighted ? '#F3F724' : '#30363d'
      ctx.lineWidth = Math.min(4, 1 + edge.callCount * 0.5)
      ctx.beginPath()
      ctx.moveTo(fromNode.x, fromNode.y)
      ctx.lineTo(toNode.x, toNode.y)
      ctx.stroke()

      // Draw arrow
      const angle = Math.atan2(toNode.y - fromNode.y, toNode.x - fromNode.x)
      const arrowSize = 8
      const arrowX = toNode.x - 30 * Math.cos(angle)
      const arrowY = toNode.y - 30 * Math.sin(angle)

      ctx.fillStyle = isHighlighted ? '#F3F724' : '#30363d'
      ctx.beginPath()
      ctx.moveTo(arrowX, arrowY)
      ctx.lineTo(
        arrowX - arrowSize * Math.cos(angle - Math.PI / 6),
        arrowY - arrowSize * Math.sin(angle - Math.PI / 6),
      )
      ctx.lineTo(
        arrowX - arrowSize * Math.cos(angle + Math.PI / 6),
        arrowY - arrowSize * Math.sin(angle + Math.PI / 6),
      )
      ctx.closePath()
      ctx.fill()

      // Draw edge label
      if (isHighlighted) {
        const midX = (fromNode.x + toNode.x) / 2
        const midY = (fromNode.y + toNode.y) / 2
        ctx.fillStyle = '#F3F724'
        ctx.font = '10px JetBrains Mono, monospace'
        ctx.textAlign = 'center'
        ctx.fillText(formatDuration(edge.totalDuration), midX, midY - 8)
        ctx.fillText(`${edge.callCount} calls`, midX, midY + 8)
      }
    })

    // Draw nodes
    nodes.forEach((node) => {
      const isHovered = hoveredNode?.id === node.id
      const isSelected = selectedNode?.id === node.id
      const nodeRadius = Math.min(35, 20 + node.spanCount * 2)

      // Node circle
      ctx.beginPath()
      ctx.arc(node.x, node.y, nodeRadius, 0, 2 * Math.PI)

      // Fill with gradient
      const gradient = ctx.createRadialGradient(
        node.x - nodeRadius * 0.3,
        node.y - nodeRadius * 0.3,
        0,
        node.x,
        node.y,
        nodeRadius,
      )
      gradient.addColorStop(0, `${node.color}ff`)
      gradient.addColorStop(1, `${node.color}99`)
      ctx.fillStyle = gradient
      ctx.fill()

      // Border
      if (isHovered || isSelected) {
        ctx.strokeStyle = '#F3F724'
        ctx.lineWidth = 3
        ctx.stroke()
      }

      // Error indicator
      if (node.errorCount > 0) {
        ctx.beginPath()
        ctx.arc(node.x + nodeRadius * 0.7, node.y - nodeRadius * 0.7, 8, 0, 2 * Math.PI)
        ctx.fillStyle = '#EF4444'
        ctx.fill()
        ctx.fillStyle = '#fff'
        ctx.font = 'bold 10px JetBrains Mono, monospace'
        ctx.textAlign = 'center'
        ctx.textBaseline = 'middle'
        ctx.fillText(String(node.errorCount), node.x + nodeRadius * 0.7, node.y - nodeRadius * 0.7)
      }

      // Node label
      ctx.fillStyle = '#F4F4F4'
      ctx.font = 'bold 11px JetBrains Mono, monospace'
      ctx.textAlign = 'center'
      ctx.textBaseline = 'middle'

      // Truncate long names
      let displayName = node.name
      while (ctx.measureText(displayName).width > nodeRadius * 1.6 && displayName.length > 3) {
        displayName = `${displayName.slice(0, -4)}...`
      }
      ctx.fillText(displayName, node.x, node.y)
    })
  }, [nodes, edges, hoveredNode, selectedNode])

  useEffect(() => {
    draw()

    const handleResize = () => draw()
    window.addEventListener('resize', handleResize)
    return () => window.removeEventListener('resize', handleResize)
  }, [draw])

  const handleMouseMove = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const canvas = canvasRef.current
    if (!canvas) return

    const rect = canvas.getBoundingClientRect()
    const x = e.clientX - rect.left
    const y = e.clientY - rect.top

    // Find hovered node
    let found: ServiceNode | null = null
    for (const node of nodes) {
      const nodeRadius = Math.min(35, 20 + node.spanCount * 2)
      const dist = Math.sqrt((x - node.x) ** 2 + (y - node.y) ** 2)
      if (dist <= nodeRadius) {
        found = node
        break
      }
    }

    setHoveredNode(found)
    setTooltipPos({ x: e.clientX, y: e.clientY })
  }

  const handleClick = () => {
    if (hoveredNode) {
      setSelectedNode(hoveredNode === selectedNode ? null : hoveredNode)

      // Find a span from this service and click it
      const span = data.spans.find((s) => {
        const service = s.service_name || s.name.split('.')[0] || 'unknown'
        return service === hoveredNode.id
      })
      if (span) {
        onSpanClick(span)
      }
    }
  }

  return (
    <div className="flex flex-col h-full">
      {/* Toolbar */}
      <div className="flex items-center justify-between px-4 py-2 border-b border-[#1D1D1D] bg-[#141414]/30 flex-shrink-0">
        <div className="flex items-center gap-2 text-xs text-gray-400">
          <span>{nodes.length} services</span>
          <span className="text-[#1D1D1D]">•</span>
          <span>{edges.length} connections</span>
        </div>
        <div className="text-xs text-gray-400">Click a node to view service details</div>
      </div>

      {/* Canvas */}
      <div ref={containerRef} className="flex-1 overflow-hidden relative">
        <canvas
          ref={canvasRef}
          onMouseMove={handleMouseMove}
          onMouseLeave={() => setHoveredNode(null)}
          onClick={handleClick}
          className="cursor-pointer"
        />

        {/* Tooltip */}
        {hoveredNode && (
          <div
            className="fixed z-50 px-3 py-2 bg-[#141414] border border-[#1D1D1D] rounded-lg shadow-xl pointer-events-none"
            style={{
              left: tooltipPos.x + 12,
              top: tooltipPos.y + 12,
            }}
          >
            <div className="flex items-center gap-2 mb-2">
              <span className="w-3 h-3 rounded-sm" style={{ backgroundColor: hoveredNode.color }} />
              <span className="text-sm font-semibold text-[#F4F4F4]">{hoveredNode.name}</span>
            </div>
            <div className="grid grid-cols-2 gap-x-4 gap-y-1 text-xs">
              <span className="text-gray-400">Spans:</span>
              <span className="text-[#F4F4F4] font-mono">{hoveredNode.spanCount}</span>
              <span className="text-gray-400">Duration:</span>
              <span className="text-[#F4F4F4] font-mono">
                {formatDuration(hoveredNode.totalDuration)}
              </span>
              {hoveredNode.errorCount > 0 && (
                <>
                  <span className="text-gray-400">Errors:</span>
                  <span className="text-[#EF4444] font-mono">{hoveredNode.errorCount}</span>
                </>
              )}
            </div>
          </div>
        )}
      </div>

      {/* Legend */}
      <div className="px-4 py-2 border-t border-[#1D1D1D] bg-[#141414]/30 flex items-center gap-6 text-xs text-gray-400 flex-shrink-0">
        <span className="font-medium">Services:</span>
        {nodes.slice(0, 6).map((node) => (
          <span key={node.id} className="flex items-center gap-1.5">
            <span className="w-2.5 h-2.5 rounded-sm" style={{ backgroundColor: node.color }} />
            {node.name}
          </span>
        ))}
        {nodes.length > 6 && <span>+{nodes.length - 6} more</span>}
      </div>
    </div>
  )
}
