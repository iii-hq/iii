import { memo } from 'react'
import { SearchBar } from './search-bar'
import { TraceEmptyState } from './trace-empty-state'
import { TraceTimeline } from './trace-timeline'
import { TracesGroups } from './traces-groups'

export const ObservabilityPage = memo(() => {
  return (
    <div className="grid grid-rows-[auto_1fr] h-full">
      <SearchBar />

      <div className="grid grid-cols-[300px_1fr] overflow-hidden">
        <div className="w-[300px] border-r border-border overflow-auto h-full" data-testid="traces-container">
          <TracesGroups />
        </div>

        <div className="overflow-auto" data-testid="trace-details">
          <TraceTimeline />
          <TraceEmptyState />
        </div>
      </div>
    </div>
  )
})
ObservabilityPage.displayName = 'ObservabilityPage'
