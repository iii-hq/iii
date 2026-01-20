import { useStreamGroup } from '@motiadev/stream-client-react'
import { useObservabilityStore } from '../stores/use-observability-store'
import type { TraceGroupMeta } from '../types/observability'

const streamName = 'motia-trace-group'
const groupId = 'default'

export const useTraceGroupsStream = () => {
  const setData = useObservabilityStore((state) => state.setTraceGroupMetas)

  useStreamGroup<TraceGroupMeta>({
    streamName,
    groupId,
    setData,
  })
}
