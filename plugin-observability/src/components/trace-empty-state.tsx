import { memo } from 'react'
import { useObservabilityStore } from '../stores/use-observability-store'

export const TraceEmptyState: React.FC = memo(() => {
  const selectedGroupId = useObservabilityStore((state) => state.selectedTraceGroupId)

  if (selectedGroupId) {
    return null
  }

  return (
    <div className="flex items-center justify-center h-full text-muted-foreground text-center">
      Select a trace or trace group to view the timeline
    </div>
  )
})
TraceEmptyState.displayName = 'TraceEmptyState'
