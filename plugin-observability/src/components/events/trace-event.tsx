import type React from 'react'
import { memo } from 'react'
import type { TraceEvent as TraceEventType } from '../../types/observability'
import { TraceEmitEvent } from './trace-emit-event'
import { TraceLogEvent } from './trace-log-event'
import { TraceStateEvent } from './trace-state-event'
import { TraceStreamEvent } from './trace-stream-event'

export const TraceEvent: React.FC<{ event: TraceEventType }> = memo(({ event }) => {
  if (event.type === 'log') {
    return <TraceLogEvent event={event} />
  } else if (event.type === 'emit') {
    return <TraceEmitEvent event={event} />
  } else if (event.type === 'state') {
    return <TraceStateEvent event={event} />
  } else if (event.type === 'stream') {
    return <TraceStreamEvent event={event} />
  }
})
TraceEvent.displayName = 'TraceEvent'
