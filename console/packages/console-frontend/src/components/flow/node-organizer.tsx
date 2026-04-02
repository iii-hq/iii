import { type Edge, type Node, useNodesInitialized, useReactFlow } from '@xyflow/react'
import * as dagre from 'dagre'
import type React from 'react'
import { useEffect, useRef } from 'react'

export function organizeNodes<T extends Node>(nodes: T[], edges: Edge[], force?: boolean): T[] {
  const g = new dagre.graphlib.Graph({ directed: true, compound: false, multigraph: false })
  g.setDefaultEdgeLabel(() => ({}))
  g.setGraph({ rankdir: 'LR', ranksep: force ? 80 : 0, nodesep: force ? 40 : 20, edgesep: 0 })

  for (const node of nodes) {
    if (!force && (node.position.x !== 0 || node.position.y !== 0)) {
      g.setNode(node.id, {
        width: node.measured?.width ?? 200,
        height: node.measured?.height ?? 100,
        x: node.position.x,
        y: node.position.y,
      })
    } else {
      g.setNode(node.id, {
        width: node.measured?.width ?? 200,
        height: node.measured?.height ?? 100,
      })
    }
  }

  for (const edge of edges) {
    g.setEdge(edge.source, edge.target)
  }

  dagre.layout(g)

  return nodes.map((node) => {
    if (!force && (node.position.x !== 0 || node.position.y !== 0)) return node

    const pos = g.node(node.id)
    return {
      ...node,
      position: {
        x: pos.x - (node.measured?.width ?? 200) / 2,
        y: pos.y - (node.measured?.height ?? 100) / 2,
      },
    }
  })
}

type Props<T extends Node = Node> = {
  onInitialized: () => void
  nodes: T[]
  edges: Edge[]
  setNodes: React.Dispatch<React.SetStateAction<T[]>>
}

export function NodeOrganizer<T extends Node>({ onInitialized, nodes, edges, setNodes }: Props<T>) {
  const { fitView } = useReactFlow()
  const nodesInitialized = useNodesInitialized()
  const initialized = useRef(false)
  const layoutDone = useRef(false)
  const lastNodeIds = useRef('')

  useEffect(() => {
    if (!nodesInitialized) return

    // Track which set of nodes we've laid out by their IDs
    const nodeIds = nodes.map((n) => n.id).join(',')
    if (nodeIds !== lastNodeIds.current) {
      lastNodeIds.current = nodeIds
      layoutDone.current = false
    }

    if (layoutDone.current) return

    const needsLayout = nodes.some((n) => n.position.x === 0 && n.position.y === 0)
    if (needsLayout) {
      layoutDone.current = true
      setNodes(organizeNodes(nodes, edges))
    } else {
      layoutDone.current = true
    }

    if (!initialized.current) {
      initialized.current = true
      onInitialized()
      setTimeout(() => fitView(), 1)
    }
  }, [nodesInitialized, onInitialized, setNodes, fitView, nodes, edges])

  return null
}
