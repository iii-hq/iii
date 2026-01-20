import { useStreamGroup } from '@motiadev/stream-client-react'
import { useCallback, useEffect } from 'react'
import { useBullMQStore } from '../stores/use-bullmq-store'
import type { QueueInfo } from '../types/queue'

const STREAM_NAME = '__motia.bullmq-queues'

type StreamQueueInfo = QueueInfo & { id: string }

export const useQueues = () => {
  const { queues, setQueues, setError, error, selectedQueue, setSelectedQueue } = useBullMQStore()

  const { data: streamQueues } = useStreamGroup<StreamQueueInfo>({
    streamName: STREAM_NAME,
    groupId: 'default',
  })

  useEffect(() => {
    if (streamQueues.length > 0) {
      setQueues(streamQueues)

      if (selectedQueue) {
        const updatedQueue = streamQueues.find((q) => q.name === selectedQueue.name)
        if (updatedQueue) {
          const currentStats = JSON.stringify(selectedQueue.stats)
          const newStats = JSON.stringify(updatedQueue.stats)
          const pausedChanged = selectedQueue.isPaused !== updatedQueue.isPaused

          if (currentStats !== newStats || pausedChanged) {
            setSelectedQueue(updatedQueue)
          }
        }
      }
    }
  }, [streamQueues, setQueues, selectedQueue, setSelectedQueue])

  const refreshQueue = useCallback(async (name: string): Promise<QueueInfo | null> => {
    try {
      const response = await fetch(`/__motia/bullmq/queues/${encodeURIComponent(name)}`)
      if (!response.ok) {
        throw new Error('Failed to fetch queue')
      }
      return await response.json()
    } catch {
      return null
    }
  }, [])

  const pauseQueue = useCallback(
    async (name: string) => {
      try {
        await fetch(`/__motia/bullmq/queues/${encodeURIComponent(name)}/pause`, { method: 'POST' })
      } catch (err) {
        setError(err instanceof Error ? err.message : 'Failed to pause queue')
      }
    },
    [setError],
  )

  const resumeQueue = useCallback(
    async (name: string) => {
      try {
        await fetch(`/__motia/bullmq/queues/${encodeURIComponent(name)}/resume`, { method: 'POST' })
      } catch (err) {
        setError(err instanceof Error ? err.message : 'Failed to resume queue')
      }
    },
    [setError],
  )

  const cleanQueue = useCallback(
    async (name: string, status: string, grace = 0, limit = 1000) => {
      try {
        await fetch(`/__motia/bullmq/queues/${encodeURIComponent(name)}/clean`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ status, grace, limit }),
        })
      } catch (err) {
        setError(err instanceof Error ? err.message : 'Failed to clean queue')
      }
    },
    [setError],
  )

  const drainQueue = useCallback(
    async (name: string) => {
      try {
        await fetch(`/__motia/bullmq/queues/${encodeURIComponent(name)}/drain`, { method: 'POST' })
      } catch (err) {
        setError(err instanceof Error ? err.message : 'Failed to drain queue')
      }
    },
    [setError],
  )

  return {
    queues,
    error,
    refreshQueue,
    pauseQueue,
    resumeQueue,
    cleanQueue,
    drainQueue,
  }
}
