import type React from 'react'
import { memo, useMemo } from 'react'
import type { StateEvent } from '../../types/observability'
import { FunctionCall } from './code/function-call'
export const TraceStateEvent: React.FC<{ event: StateEvent }> = memo(({ event }) => {
  const args = useMemo(() => [event.data.traceId, event.data.key, event.data.value], [event])
  return <FunctionCall objectName="state" functionName={event.operation} args={args} />
})
TraceStateEvent.displayName = 'TraceStateEvent'
