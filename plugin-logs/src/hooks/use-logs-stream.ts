import { useStreamGroup } from '@motiadev/stream-client-react'
import { useLogsStore } from '../stores/use-logs-store'
import type { Log } from '../types/log'

const streamName = '__motia.logs'
const groupId = 'default'

export const useLogsStream = () => {
  const setData = useLogsStore((state) => state.setLogs)

  useStreamGroup<Log>({
    streamName,
    groupId,
    setData,
  })
}
