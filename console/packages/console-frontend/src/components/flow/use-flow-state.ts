import {
  type Edge,
  type Node,
  type OnNodesChange,
  useEdgesState,
  useNodesState,
} from '@xyflow/react'
import { useCallback, useEffect, useMemo, useRef } from 'react'
import { saveFlowConfig } from '../../api/flows/flows'
import type { FlowConfigResponse, FlowResponse, NodeConfig, NodeData } from '../../api/flows/types'
import { ApiFlowNode } from './nodes/api-node'
import { CronFlowNode } from './nodes/cron-node'
import { EventFlowNode } from './nodes/event-node'
import { NoopFlowNode } from './nodes/noop-node'
import { QueueFlowNode } from './nodes/queue-node'
import { StateFlowNode } from './nodes/state-node'

const DEFAULT_CONFIG: NodeConfig = { x: 0, y: 0 }

const NODE_TYPES = {
  event: EventFlowNode,
  http: ApiFlowNode,
  api: ApiFlowNode, // fallback
  noop: NoopFlowNode,
  cron: CronFlowNode,
  queue: QueueFlowNode, // fallback
  'durable:subscriber': QueueFlowNode,
  state: StateFlowNode,
}

function buildNodes(flow: FlowResponse, flowConfig: FlowConfigResponse): Node<NodeData>[] {
  return flow.steps.map((step) => {
    const cfg = step.filePath
      ? (flowConfig?.config[step.filePath] ?? DEFAULT_CONFIG)
      : DEFAULT_CONFIG
    return {
      id: step.id,
      type: step.type,
      position: { x: cfg.x, y: cfg.y },
      data: { ...step, nodeConfig: cfg },
    }
  })
}

function buildEdges(flow: FlowResponse): Edge[] {
  return flow.edges.map((edge) => ({
    ...edge,
    type: 'base',
  }))
}

export function useFlowState(flow: FlowResponse, flowConfig: FlowConfigResponse) {
  const [nodes, setNodes, onNodesChange] = useNodesState<Node<NodeData>>([])
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([])

  const saveTimeoutRef = useRef<NodeJS.Timeout | null>(null)
  const lastSyncKeyRef = useRef<string>('')
  const isUserDragRef = useRef(false)

  const savePositions = useCallback(() => {
    setNodes((currentNodes) => {
      const config = currentNodes.reduce<FlowConfigResponse['config']>((acc, node) => {
        const data = node.data as NodeData
        if (data.filePath) {
          acc[data.filePath] = {
            x: Math.round(node.position.x),
            y: Math.round(node.position.y),
          }
          if (data.nodeConfig?.sourceHandlePosition) {
            acc[data.filePath].sourceHandlePosition = data.nodeConfig.sourceHandlePosition
          }
          if (data.nodeConfig?.targetHandlePosition) {
            acc[data.filePath].targetHandlePosition = data.nodeConfig.targetHandlePosition
          }
        }
        return acc
      }, {})

      saveFlowConfig(flow.id, { id: flow.id, config }).catch((err) =>
        console.error('Failed to save flow config:', err),
      )

      return currentNodes
    })
  }, [flow.id, setNodes])

  // Memoize config serialization -- only recomputes when flowConfig reference changes
  const configStr = useMemo(() => JSON.stringify(flowConfig?.config ?? {}), [flowConfig])

  // Sync nodes/edges from flow data - only when data actually changes
  useEffect(() => {
    if (!flow) return

    const syncKey = `${flow.id}:${flow.steps.length}:${flow.edges.length}:${configStr}`
    if (syncKey === lastSyncKeyRef.current) return
    lastSyncKeyRef.current = syncKey

    setNodes(buildNodes(flow, flowConfig))
    setEdges(buildEdges(flow))
  }, [flow, flowConfig, configStr, setNodes, setEdges])

  // Wrap onNodesChange to detect user-initiated drags
  const handleNodesChange: OnNodesChange<Node<NodeData>> = useCallback(
    (changes) => {
      const hasDrag = changes.some((c) => c.type === 'position' && c.dragging)
      const hasDragEnd = changes.some((c) => c.type === 'position' && !c.dragging)

      if (hasDrag) {
        isUserDragRef.current = true
      }

      onNodesChange(changes)

      // Save config only after user finishes dragging
      if (hasDragEnd && isUserDragRef.current) {
        isUserDragRef.current = false

        if (saveTimeoutRef.current) clearTimeout(saveTimeoutRef.current)
        saveTimeoutRef.current = setTimeout(() => savePositions(), 300)
      }
    },
    [onNodesChange, savePositions],
  )

  // Cleanup timeout on unmount -- flush pending save
  useEffect(() => {
    return () => {
      if (saveTimeoutRef.current) {
        clearTimeout(saveTimeoutRef.current)
        savePositions()
      }
    }
  }, [savePositions])

  return useMemo(
    () => ({
      nodes,
      edges,
      setNodes,
      onNodesChange: handleNodesChange,
      onEdgesChange,
      nodeTypes: NODE_TYPES,
      savePositions,
    }),
    [nodes, edges, setNodes, handleNodesChange, onEdgesChange, savePositions],
  )
}
