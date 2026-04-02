import { useQuery, useQueryClient } from '@tanstack/react-query'
import { ReactFlowProvider } from '@xyflow/react'
import {
  Activity,
  ChevronRight,
  GitBranch,
  LayoutGrid,
  Maximize,
  RefreshCw,
  Workflow,
} from 'lucide-react'
import { Component, useCallback, useRef, useState } from 'react'
import { flowConfigQuery, flowsQuery } from '../../api/queries'
import { Badge, Button } from '../ui/card'
import { Tooltip } from '../ui/tooltip'
import { FlowSelector } from './flow-selector'
import { FlowView, type FlowViewHandle } from './flow-view'

class FlowErrorBoundary extends Component<
  { children: React.ReactNode; onReset?: () => void },
  { hasError: boolean }
> {
  state = { hasError: false }
  static getDerivedStateFromError() {
    return { hasError: true }
  }
  render() {
    if (this.state.hasError) {
      return (
        <div className="flex flex-col items-center justify-center h-full gap-4">
          <div className="p-4 rounded-full bg-[#141414] border border-[#1D1D1D]">
            <Workflow className="w-8 h-8 text-muted/30" />
          </div>
          <div className="text-center">
            <h3 className="text-sm font-medium text-foreground mb-1">
              Something went wrong rendering this flow
            </h3>
            <p className="text-xs text-muted max-w-xs mb-3">
              An unexpected error occurred. Try refreshing the page.
            </p>
            <button
              type="button"
              onClick={() => {
                this.setState({ hasError: false })
                this.props.onReset?.()
              }}
              className="text-xs text-[#F3F724] hover:underline"
            >
              Try again
            </button>
          </div>
        </div>
      )
    }
    return this.props.children
  }
}

export function FlowPage() {
  const queryClient = useQueryClient()
  const { data: flows, isLoading, isFetching } = useQuery(flowsQuery())
  const [selectedFlowId, setSelectedFlowId] = useState<string | null>(null)
  const flowViewRef = useRef<FlowViewHandle>(null)

  const activeFlowId = selectedFlowId ?? flows?.[0]?.id ?? ''
  const activeFlow = flows?.find((f) => f.id === activeFlowId)

  const { data: flowConfig } = useQuery(flowConfigQuery(activeFlowId))

  const handleRefresh = useCallback(() => {
    queryClient.invalidateQueries({ queryKey: ['flows'] })
    if (activeFlowId) {
      queryClient.invalidateQueries({ queryKey: ['flowConfig', activeFlowId] })
    }
  }, [queryClient, activeFlowId])

  const handleFitView = useCallback(() => {
    flowViewRef.current?.fitView()
  }, [])

  const handleAutoOrganize = useCallback(() => {
    flowViewRef.current?.autoOrganize()
  }, [])

  const stepCount = activeFlow?.steps.length ?? 0
  const edgeCount = activeFlow?.edges.length ?? 0

  return (
    <div className="flex flex-col h-full bg-background text-foreground">
      {/* Header */}
      <div className="flex items-center justify-between px-3 md:px-5 py-2 md:py-2.5 bg-dark-gray/30 border-b border-border">
        {/* Left: Flow identity */}
        <div className="flex items-center gap-1 md:gap-1.5 min-w-0">
          <div className="flex items-center gap-1.5 text-[#5B5B5B] shrink-0">
            <Workflow className="w-4 h-4" />
            <span className="text-[10px] md:text-xs font-medium uppercase tracking-wider">
              Flow
            </span>
          </div>

          {activeFlow && (
            <>
              <ChevronRight className="w-3 h-3 text-[#5B5B5B]/50 shrink-0" />

              {flows && flows.length > 1 ? (
                <FlowSelector
                  flows={flows}
                  selectedFlowId={activeFlowId}
                  onSelectFlow={setSelectedFlowId}
                />
              ) : (
                <span className="text-xs md:text-sm font-medium text-[#F4F4F4] tracking-wide truncate">
                  {activeFlow.name}
                </span>
              )}

              <div className="hidden sm:flex items-center gap-1.5 ml-2 pl-2.5 border-l border-[#1D1D1D]">
                <Badge variant="success" className="gap-1 text-[10px]">
                  <Activity className="w-2.5 h-2.5" />
                  {stepCount}
                </Badge>
                <Badge variant="default" className="gap-1 text-[10px]">
                  <GitBranch className="w-2.5 h-2.5" />
                  {edgeCount}
                </Badge>
              </div>
            </>
          )}
        </div>

        {/* Right: Actions */}
        <div className="flex items-center gap-0.5 shrink-0">
          {activeFlow && (
            <>
              <Tooltip label="Fit to view">
                <Button variant="ghost" size="sm" onClick={handleFitView}>
                  <Maximize className="w-3.5 h-3.5" />
                </Button>
              </Tooltip>
              <Tooltip label="Auto-organize">
                <Button variant="ghost" size="sm" onClick={handleAutoOrganize}>
                  <LayoutGrid className="w-3.5 h-3.5" />
                </Button>
              </Tooltip>
              <div className="w-px h-4 bg-[#1D1D1D] mx-1" />
            </>
          )}
          <Tooltip label="Refresh">
            <Button variant="ghost" size="sm" onClick={handleRefresh} disabled={isFetching}>
              <RefreshCw className={`w-3.5 h-3.5 ${isFetching ? 'animate-spin' : ''}`} />
            </Button>
          </Tooltip>
        </div>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-hidden relative">
        {isLoading ? (
          <div className="flex items-center justify-center h-full">
            <div className="flex items-center gap-3 text-muted">
              <RefreshCw className="w-5 h-5 animate-spin" />
              <span className="text-xs uppercase tracking-wider">Loading flows...</span>
            </div>
          </div>
        ) : !flows || flows.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-full gap-4">
            <div className="p-4 rounded-full bg-[#141414] border border-[#1D1D1D]">
              <Workflow className="w-8 h-8 text-muted/30" />
            </div>
            <div className="text-center">
              <h3 className="text-sm font-medium text-foreground mb-1">No flows registered</h3>
              <p className="text-xs text-muted max-w-xs">
                Flows will appear here once functions with flow metadata are registered with the
                engine.
              </p>
            </div>
          </div>
        ) : !activeFlow || !flowConfig ? (
          <div className="flex items-center justify-center h-full">
            <div className="flex items-center gap-3 text-muted">
              <RefreshCw className="w-5 h-5 animate-spin" />
              <span className="text-xs uppercase tracking-wider">Loading flow config...</span>
            </div>
          </div>
        ) : (
          <FlowErrorBoundary onReset={handleRefresh}>
            <ReactFlowProvider key={activeFlowId}>
              <FlowView ref={flowViewRef} flow={activeFlow} flowConfig={flowConfig} />
            </ReactFlowProvider>
          </FlowErrorBoundary>
        )}
      </div>
    </div>
  )
}
