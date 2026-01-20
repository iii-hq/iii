import type React from 'react'
import { Fragment, memo } from 'react'
import { formatDuration } from '../../lib/utils'
import type { TraceEvent as TraceEventType } from '../../types/observability'
import { EventIcon } from '../events/event-icon'
import { TraceEvent } from '../events/trace-event'

type Props = {
  event: TraceEventType
  traceStartTime: number
}

export const TraceEventItem: React.FC<Props> = memo(({ event, traceStartTime }) => {
  return (
    <Fragment>
      <div className="grid place-items-center">
        <div className="w-1 h-1 rounded-full bg-emerald-500 outline outline-2 outline-emerald-500/50 -ml-[26px]"></div>
      </div>
      <div className="grid place-items-center">
        <EventIcon event={event} />
      </div>
      <div className="grid place-items-center">
        <span className="text-sm font-mono text-muted-foreground">
          +{formatDuration(Math.floor(event.timestamp - traceStartTime))}
        </span>
      </div>
      <div className="grid place-items-start">
        <TraceEvent event={event} />
      </div>
    </Fragment>
  )
})
TraceEventItem.displayName = 'TraceEventItem'
