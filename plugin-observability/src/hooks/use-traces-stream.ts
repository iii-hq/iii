import { useStreamGroup } from '@motiadev/stream-client-react'
import { useEffect, useRef } from 'react'
import { useObservabilityStore } from '../stores/use-observability-store'
import type { Trace } from '../types/observability'

const streamName = 'motia-trace'

export const useTracesStream = () => {
  const groupId = useObservabilityStore((state) => state.selectedTraceGroupId)
  const setData = useObservabilityStore((state) => state.setTraces)
  const setTracesForGroup = useObservabilityStore((state) => state.setTracesForGroup)
  const groupIdRef = useRef(groupId)

  useEffect(() => {
    groupIdRef.current = groupId
  }, [groupId])

  useStreamGroup<Trace>({
    streamName,
    groupId,
    setData: (traces: Trace[]) => {
      const currentGroupId = groupIdRef.current
      setData(traces)
      if (currentGroupId) {
        setTracesForGroup(currentGroupId, traces)
      }
    },
  })
}
