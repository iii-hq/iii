import { useVirtualizer } from '@tanstack/react-virtual'
import type React from 'react'
import { memo, useEffect, useMemo, useRef } from 'react'
import { useAllTracesStream } from '../hooks/use-all-traces-stream'
import { useFilteredTraceGroups } from '../hooks/use-filtered-trace-groups'
import { useTraceGroupsStream } from '../hooks/use-trace-groups-stream'
import { useObservabilityStore } from '../stores/use-observability-store'
import { TraceGroupItem } from './trace-group-item'

const ROW_HEIGHT = 110

export const TracesGroups: React.FC = memo(() => {
  useTraceGroupsStream()
  useAllTracesStream()

  const groups = useFilteredTraceGroups()
  const selectedGroupId = useObservabilityStore((state) => state.selectedTraceGroupId)
  const selectTraceGroupId = useObservabilityStore((state) => state.selectTraceGroupId)
  const scrollContainerRef = useRef<HTMLDivElement>(null)

  const groupsLength = useMemo(() => groups?.length || 0, [groups])
  const lastRunningGroupId = useMemo(() => {
    if (!groups || groups.length === 0) return ''
    const lastGroup = groups[groups.length - 1]
    return lastGroup?.status === 'running' ? lastGroup.id : ''
  }, [groups])

  useEffect(() => {
    if (lastRunningGroupId && lastRunningGroupId !== selectedGroupId) {
      selectTraceGroupId(lastRunningGroupId)
    } else if (!lastRunningGroupId && !groupsLength && selectedGroupId) {
      selectTraceGroupId('')
    }
  }, [lastRunningGroupId, groupsLength, selectedGroupId, selectTraceGroupId])

  const reversedGroups = useMemo(() => (groups ? [...groups].reverse() : []), [groups])

  const virtualizer = useVirtualizer({
    count: reversedGroups.length,
    getScrollElement: () => scrollContainerRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 5,
  })

  const virtualItems = virtualizer.getVirtualItems()

  if (!groups || groups.length === 0) {
    return null
  }

  return (
    <div ref={scrollContainerRef} className="overflow-auto h-full">
      <div className="relative w-full" style={{ height: virtualizer.getTotalSize() }}>
        {virtualItems.map((virtualRow) => {
          const group = reversedGroups[virtualRow.index]
          if (!group) return null
          return (
            <TraceGroupItem
              key={group.id}
              groupId={group.id}
              groupName={group.name}
              groupStatus={group.status}
              groupStartTime={group.startTime}
              groupEndTime={group.endTime}
              totalSteps={group.metadata.totalSteps}
              activeSteps={group.metadata.activeSteps}
              isSelected={selectedGroupId === group.id}
              style={{
                position: 'absolute',
                top: 0,
                left: 0,
                width: '100%',
                height: ROW_HEIGHT,
                transform: `translateY(${virtualRow.start}px)`,
              }}
            />
          )
        })}
      </div>
    </div>
  )
})
TracesGroups.displayName = 'TracesGroups'
