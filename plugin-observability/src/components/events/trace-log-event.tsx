import { LevelDot } from '@motiadev/ui'
import type React from 'react'
import { memo } from 'react'
import type { LogEntry } from '../../types/observability'

export const TraceLogEvent: React.FC<{ event: LogEntry }> = memo(({ event }) => {
  return (
    <div className="flex items-center gap-2">
      <LevelDot level={event.level} /> {event.message}
    </div>
  )
})
TraceLogEvent.displayName = 'TraceLogEvent'
