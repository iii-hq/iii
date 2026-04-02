import { Background, BackgroundVariant, MiniMap, ReactFlow, useReactFlow } from '@xyflow/react'
import { forwardRef, useCallback, useImperativeHandle, useState } from 'react'
import type { FlowConfigResponse, FlowResponse } from '../../api/flows/types'
import { FlowEdge } from './base-edge'
import { NodeOrganizer, organizeNodes } from './node-organizer'
import { useFlowState } from './use-flow-state'

import '@xyflow/react/dist/style.css'

const edgeTypes = { base: FlowEdge }
const proOptions = { hideAttribution: true }

export type FlowViewHandle = {
  fitView: () => void
  autoOrganize: () => void
}

type Props = {
  flow: FlowResponse
  flowConfig: FlowConfigResponse
}

export const FlowView = forwardRef<FlowViewHandle, Props>(function FlowView(
  { flow, flowConfig },
  ref,
) {
  const { nodes, edges, setNodes, onNodesChange, onEdgesChange, nodeTypes, savePositions } =
    useFlowState(flow, flowConfig)
  const { fitView } = useReactFlow()
  const [initialized, setInitialized] = useState(false)
  const onInitialized = useCallback(() => setInitialized(true), [])

  const handleFitView = useCallback(() => {
    fitView({ padding: 0.1, duration: 200 })
  }, [fitView])

  const handleAutoOrganize = useCallback(() => {
    setNodes((currentNodes) => organizeNodes(currentNodes, edges, true))
    savePositions()
    setTimeout(() => fitView({ padding: 0.1, duration: 200 }), 50)
  }, [setNodes, edges, savePositions, fitView])

  useImperativeHandle(
    ref,
    () => ({
      fitView: handleFitView,
      autoOrganize: handleAutoOrganize,
    }),
    [handleFitView, handleAutoOrganize],
  )

  return (
    <div className="w-full h-full relative">
      {!initialized && (
        <div className="absolute inset-0 z-10 flex items-center justify-center bg-background">
          <div className="flex items-center gap-3 text-muted">
            <div className="w-5 h-5 border-2 border-yellow/30 border-t-yellow rounded-full animate-spin" />
            <span className="text-xs uppercase tracking-wider">Laying out nodes...</span>
          </div>
        </div>
      )}
      <ReactFlow
        minZoom={0.1}
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        edgeTypes={edgeTypes}
        onNodesChange={onNodesChange}
        onEdgesChange={onEdgesChange}
        className="isolate"
        proOptions={proOptions}
      >
        <Background variant={BackgroundVariant.Dots} gap={20} size={1} color="#1D1D1D" />
        <MiniMap
          style={{ background: '#141414', borderRadius: 4 }}
          maskColor="rgba(0, 0, 0, 0.6)"
          nodeColor="#1D1D1D"
          nodeBorderRadius={4}
        />
        <NodeOrganizer
          onInitialized={onInitialized}
          nodes={nodes}
          edges={edges}
          setNodes={setNodes}
        />
      </ReactFlow>
    </div>
  )
})
