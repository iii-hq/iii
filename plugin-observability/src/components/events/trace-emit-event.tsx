import type React from 'react'
import { memo, useMemo } from 'react'
import type { EmitEvent } from '../../types/observability'
import { FunctionCall } from './code/function-call'

export const TraceEmitEvent: React.FC<{ event: EmitEvent }> = memo(({ event }) => {
  const args = useMemo(() => [{ topic: event.topic, data: event.data }], [event])
  return <FunctionCall functionName="emit" args={args} />
})
TraceEmitEvent.displayName = 'TraceEmitEvent'
