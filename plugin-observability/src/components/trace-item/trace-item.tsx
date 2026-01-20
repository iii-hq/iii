import { cn } from '@motiadev/ui'
import type React from 'react'
import { memo, useCallback, useMemo } from 'react'

const GRADIENT_CLASSES = {
  running: 'bg-[repeating-linear-gradient(140deg,#BEFE29,#BEFE29_8px,#ABE625_8px,#ABE625_16px)]',
  completed: 'bg-[repeating-linear-gradient(140deg,#2862FE,#2862FE_8px,#2358E5_8px,#2358E5_16px)]',
  failed: 'bg-[repeating-linear-gradient(140deg,#EA2069,#EA2069_8px,#D41E60_8px,#D41E60_16px)]',
} as const

type Props = {
  traceId: string
  traceName: string
  traceStatus: 'running' | 'completed' | 'failed'
  traceStartTime: number
  traceEndTime: number | undefined
  groupStartTime: number
  groupEndTime: number
  onExpand: (traceId: string) => void
}

export const TraceItem: React.FC<Props> = memo(
  ({ traceId, traceName, traceStatus, traceStartTime, traceEndTime, groupStartTime, groupEndTime, onExpand }) => {
    const handleClick = useCallback(() => {
      onExpand(traceId)
    }, [onExpand, traceId])

    const barClassName = useMemo(
      () => cn('h-[24px] rounded-[4px] hover:opacity-80 transition-all duration-200', GRADIENT_CLASSES[traceStatus]),
      [traceStatus],
    )

    const barStyle = useMemo(
      () => ({
        marginLeft: `${((traceStartTime - groupStartTime) / (groupEndTime - groupStartTime)) * 100}%`,
        width: traceEndTime
          ? `${((traceEndTime - traceStartTime) / (groupEndTime - groupStartTime)) * 100}%`
          : `${((Date.now() - traceStartTime) / (groupEndTime - groupStartTime)) * 100}%`,
      }),
      [traceStartTime, traceEndTime, groupStartTime, groupEndTime],
    )

    return (
      <div
        className="flex hover:bg-muted-foreground/10 relative cursor-pointer"
        onClick={handleClick}
        data-testid="trace-timeline-item"
      >
        <div className="flex items-center min-w-[200px] max-w-[200px] h-[32px] max-h-[32px] py-4 px-2 text-sm font-semibold text-foreground sticky left-0 bg-card z-9">
          <span className="truncate min-w-0">{traceName}</span>
        </div>
        <div className="flex w-full flex-row items-center hover:bg-muted/50 rounded-md">
          <div className="relative w-full h-[32px] flex items-center">
            <div className={barClassName} style={barStyle} />
          </div>
        </div>
      </div>
    )
  },
)
TraceItem.displayName = 'TraceItem'
