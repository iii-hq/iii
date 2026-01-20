import { cn } from '@motiadev/ui'
import { formatDistanceToNow } from 'date-fns'
import type React from 'react'
import { memo, useCallback, useMemo } from 'react'
import { formatDuration } from '../lib/utils'
import { useObservabilityStore } from '../stores/use-observability-store'
import { TraceStatusBadge } from './trace-status'
import { CopyButton } from './ui/copy-button'

interface TraceGroupItemProps {
  groupId: string
  groupName: string
  groupStatus: 'running' | 'completed' | 'failed'
  groupStartTime: number
  groupEndTime: number | undefined
  totalSteps: number
  activeSteps: number
  isSelected: boolean
  style?: React.CSSProperties
}

export const TraceGroupItem: React.FC<TraceGroupItemProps> = memo(
  ({ groupId, groupName, groupStatus, groupStartTime, groupEndTime, totalSteps, activeSteps, isSelected, style }) => {
    const selectTraceGroupId = useObservabilityStore((state) => state.selectTraceGroupId)

    const duration = useMemo(
      () => (groupEndTime ? formatDuration(groupEndTime - groupStartTime) : undefined),
      [groupEndTime, groupStartTime],
    )

    const onSelect = useCallback(() => {
      selectTraceGroupId(groupId)
    }, [groupId, selectTraceGroupId])

    return (
      <div
        data-testid={`trace-${groupId}`}
        key={groupId}
        className={cn(
          'motia-trace-group cursor-pointer transition-colors w-full text-left',
          isSelected ? 'bg-muted-foreground/10' : 'hover:bg-muted/70',
        )}
        style={style}
        onClick={onSelect}
      >
        <div className="p-3 flex flex-col gap-1">
          <div className="flex flex-row justify-between items-center gap-2">
            <span className="font-semibold text-lg truncate flex-1 min-w-0">{groupName}</span>
            <TraceStatusBadge status={groupStatus} duration={duration} />
          </div>

          <div className="text-xs text-muted-foreground space-y-1">
            <div className="flex justify-between items-center">
              <div
                data-testid="trace-id"
                className="text-xs text-muted-foreground font-mono tracking-[1px] flex items-center gap-1.5 group"
              >
                {groupId}
                <CopyButton
                  textToCopy={groupId}
                  className="w-4 h-4 p-0 opacity-0 group-hover:opacity-100 transition-opacity focus:opacity-100"
                  title="Copy Trace ID"
                />
              </div>
              <span>{totalSteps} steps</span>
            </div>
            <div className="flex justify-between">{formatDistanceToNow(groupStartTime)} ago</div>
            {activeSteps > 0 && <div className="text-blue-600">{activeSteps} active</div>}
          </div>
        </div>
      </div>
    )
  },
)
TraceGroupItem.displayName = 'TraceGroupItem'
