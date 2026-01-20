import { useCallback, useEffect, useState } from 'react'
import { useStreamsStore } from '../stores/use-streams-store'
import type { StreamItem } from '../types/stream'

interface GroupItemsResponse {
  streamName: string
  groupId: string
  items: StreamItem[]
  count: number
}

interface StreamDetailsResponse {
  id: string
  name: string
  hidden: boolean
  filePath: string
  schema?: unknown
}

export const useStreamDetails = () => {
  const selectedStreamId = useStreamsStore((state) => state.selectedStreamId)
  const selectedGroupId = useStreamsStore((state) => state.selectedGroupId)
  const setSelectedGroupItems = useStreamsStore((state) => state.setSelectedGroupItems)
  const selectedGroupItems = useStreamsStore((state) => state.selectedGroupItems)

  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [streamDetails, setStreamDetails] = useState<StreamDetailsResponse | null>(null)

  const fetchStreamDetails = useCallback(async (streamName: string) => {
    setLoading(true)
    setError(null)

    try {
      const response = await fetch(`/__motia/streams/details/${encodeURIComponent(streamName)}`)
      if (!response.ok) {
        throw new Error(`Failed to fetch stream details: ${response.statusText}`)
      }

      const data: StreamDetailsResponse = await response.json()
      setStreamDetails(data)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Unknown error')
      setStreamDetails(null)
    } finally {
      setLoading(false)
    }
  }, [])

  const fetchGroupItems = useCallback(
    async (streamName: string, groupId: string) => {
      setLoading(true)
      setError(null)

      try {
        const response = await fetch(
          `/__motia/streams/group/${encodeURIComponent(streamName)}/${encodeURIComponent(groupId)}`,
        )
        if (!response.ok) {
          throw new Error(`Failed to fetch items: ${response.statusText}`)
        }

        const data: GroupItemsResponse = await response.json()
        setSelectedGroupItems(data.items || [])
      } catch (err) {
        setError(err instanceof Error ? err.message : 'Unknown error')
        setSelectedGroupItems([])
      } finally {
        setLoading(false)
      }
    },
    [setSelectedGroupItems],
  )

  useEffect(() => {
    if (selectedStreamId) {
      fetchStreamDetails(selectedStreamId)
    } else {
      setStreamDetails(null)
    }
  }, [selectedStreamId, fetchStreamDetails])

  useEffect(() => {
    if (selectedStreamId && selectedGroupId) {
      fetchGroupItems(selectedStreamId, selectedGroupId)
    }
  }, [selectedStreamId, selectedGroupId, fetchGroupItems])

  return {
    streamDetails,
    items: selectedGroupItems,
    loading,
    error,
    refetchDetails: () => selectedStreamId && fetchStreamDetails(selectedStreamId),
    refetchItems: () => selectedStreamId && selectedGroupId && fetchGroupItems(selectedStreamId, selectedGroupId),
  }
}
