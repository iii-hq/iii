import type React from 'react'
import { memo, useMemo } from 'react'
import type { StreamEvent } from '../../types/observability'
import { FunctionCall } from './code/function-call'

export const TraceStreamEvent: React.FC<{ event: StreamEvent }> = memo(({ event }) => {
  const args = useMemo(
    () => [event.data.groupId, event.data.id, event.data.data as unknown as Array<string | object | false | undefined>],
    [event],
  )
  return (
    <FunctionCall
      topLevelClassName="streams"
      objectName={event.streamName}
      functionName={event.operation}
      args={args}
      callsQuantity={event.calls}
    />
  )
})
TraceStreamEvent.displayName = 'TraceStreamEvent'
