import { useCallback, useEffect, useMemo, useState } from 'react'
import { useStreamsStore } from '../stores/use-streams-store'
import type { StreamInfo } from '../types/stream'

type StreamRegistryInfo = StreamInfo & { id: string }

export const useStreams = () => {
  const [streams, setStreams] = useState<StreamRegistryInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const search = useStreamsStore((state) => state.search)
  const showHidden = useStreamsStore((state) => state.showHidden)

  const fetchStreams = useCallback(async () => {
    setLoading(true)
    setError(null)

    try {
      const endpoint = showHidden ? '/__motia/streams/all' : '/__motia/streams'
      const response = await fetch(endpoint)
      if (!response.ok) {
        throw new Error(`Failed to fetch streams: ${response.statusText}`)
      }

      const data: { streams: StreamRegistryInfo[] } = await response.json()
      setStreams(data.streams)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Unknown error')
      setStreams([])
    } finally {
      setLoading(false)
    }
  }, [showHidden])

  useEffect(() => {
    fetchStreams()
  }, [fetchStreams])

  const filteredStreams = useMemo(() => {
    return streams.filter((stream) => {
      if (search) {
        const searchLower = search.toLowerCase()
        return stream.name.toLowerCase().includes(searchLower) || stream.filePath?.toLowerCase().includes(searchLower)
      }

      return true
    })
  }, [streams, search])

  const groupedStreams = useMemo(() => {
    const groups: Record<string, StreamRegistryInfo[]> = {}

    for (const stream of filteredStreams) {
      if (stream.name.startsWith('__motia.')) {
        groups.Internal = groups.Internal || []
        groups.Internal.push(stream)
        continue
      }

      const match = stream.name.match(/^([^.-]+)/)
      const groupName = match ? match[1].toUpperCase() : 'OTHER'

      groups[groupName] = groups[groupName] || []
      groups[groupName].push(stream)
    }

    return groups
  }, [filteredStreams])

  return {
    streams: filteredStreams,
    groupedStreams,
    totalCount: streams.length,
    visibleCount: filteredStreams.length,
    loading,
    error,
    refetch: fetchStreams,
  }
}
