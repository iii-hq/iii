import { useQuery } from '@tanstack/react-query'
import { useEffect, useRef, useState } from 'react'
import { fetchTraces } from '@/api'
import type { TracesFilterParams } from '@/api/observability/traces'
import { toMs } from '@/lib/traceTransform'

const DEFAULT_TRACE_LIMIT = 500

export interface TraceGroup {
  traceId: string
  rootOperation: string
  functionId?: string
  topic?: string
  status: 'ok' | 'error' | 'pending'
  startTime: number
  endTime?: number
  duration?: number
  spanCount: number
  services: string[]
}

export interface UseTraceDataOptions {
  filterParams: TracesFilterParams
  showSystem: boolean
  debouncedSearch: string
  isPaused: boolean
}

export interface UseTraceDataReturn {
  traceGroups: TraceGroup[]
  newTraceIds: Set<string>
  setNewTraceIds: React.Dispatch<React.SetStateAction<Set<string>>>
  hasOtelConfigured: boolean
  isQueryLoading: boolean
  refetch: () => void
  isHoveredRef: React.MutableRefObject<boolean>
  flushPendingTraces: () => void
}

export function useTraceData({
  filterParams,
  showSystem,
  debouncedSearch,
  isPaused,
}: UseTraceDataOptions): UseTraceDataReturn {
  const [traceGroups, setTraceGroups] = useState<TraceGroup[]>([])
  const [hasOtelConfigured, setHasOtelConfigured] = useState(false)
  const [newTraceIds, setNewTraceIds] = useState<Set<string>>(new Set())

  const fingerprintRef = useRef<string>('')
  const prevTraceIdsRef = useRef<Set<string>>(new Set())

  const isHoveredRef = useRef(false)
  const pendingTracesRef = useRef<TraceGroup[] | null>(null)

  const {
    data: tracesData,
    isLoading: isQueryLoading,
    refetch,
  } = useQuery({
    queryKey: ['traces', filterParams, showSystem, debouncedSearch],
    queryFn: () =>
      fetchTraces({
        ...filterParams,
        ...(debouncedSearch && !filterParams.name
          ? { name: debouncedSearch, search_all_spans: true }
          : {}),
        offset: 0,
        limit: DEFAULT_TRACE_LIMIT,
        include_internal: showSystem,
      }),
    refetchInterval: isPaused ? false : 3000,
    staleTime: 1000,
  })

  useEffect(() => {
    if (!tracesData) return

    if (tracesData.spans && tracesData.spans.length > 0) {
      const traces: TraceGroup[] = tracesData.spans.map((span) => {
        const startTime = toMs(span.start_time_unix_nano)
        const endTime = toMs(span.end_time_unix_nano)
        const duration = endTime - startTime

        const attrs: Record<string, unknown> = {}
        if (Array.isArray(span.attributes)) {
          for (const item of span.attributes) {
            if (Array.isArray(item) && item.length >= 2) {
              attrs[String(item[0])] = item[1]
            }
          }
        } else if (span.attributes) {
          Object.assign(attrs, span.attributes)
        }
        const functionId = (attrs['faas.invoked_name'] || attrs.function_id) as string | undefined
        const topic = attrs['messaging.destination.name'] as string | undefined

        return {
          traceId: span.trace_id,
          rootOperation: span.name,
          functionId,
          topic,
          status: span.status.toLowerCase() === 'error' ? 'error' : 'ok',
          startTime,
          endTime,
          duration,
          spanCount: 1,
          services: [span.service_name || 'unknown'],
        }
      })

      traces.sort((a, b) => b.startTime - a.startTime)

      const fingerprint = `${traces.length}:${traces[0]?.traceId}:${traces[traces.length - 1]?.traceId}:${traces[traces.length - 1]?.startTime}`
      if (fingerprint === fingerprintRef.current) return
      fingerprintRef.current = fingerprint

      const currentIds = new Set(traces.map((t) => t.traceId))
      if (prevTraceIdsRef.current.size > 0) {
        const freshIds = new Set<string>()
        for (const id of currentIds) {
          if (!prevTraceIdsRef.current.has(id)) freshIds.add(id)
        }
        if (freshIds.size > 0) setNewTraceIds(freshIds)
      }
      prevTraceIdsRef.current = currentIds

      if (isHoveredRef.current) {
        pendingTracesRef.current = traces
        return
      }

      setTraceGroups(traces)
      setHasOtelConfigured(true)
    } else {
      setTraceGroups([])
      setHasOtelConfigured(false)
    }
  }, [tracesData])

  const flushPendingTraces = () => {
    if (pendingTracesRef.current) {
      setTraceGroups(pendingTracesRef.current)
      setHasOtelConfigured(true)
      pendingTracesRef.current = null
    }
  }

  return {
    traceGroups,
    newTraceIds,
    setNewTraceIds,
    hasOtelConfigured,
    isQueryLoading,
    refetch,
    isHoveredRef,
    flushPendingTraces,
  }
}
