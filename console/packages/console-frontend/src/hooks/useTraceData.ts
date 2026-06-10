import { useQuery } from '@tanstack/react-query'
import { useEffect, useRef, useState } from 'react'
import { fetchTraces } from '@/api'
import type { TracesFilterParams } from '@/api/observability/traces'
import { buildTraceGroups, type TraceGroup, traceGroupsFingerprint } from '@/lib/traceGroups'

const DEFAULT_TRACE_LIMIT = 500

export type { TraceGroup } from '@/lib/traceGroups'

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
    // Interim: poll every 1s (was 3s) so freshly emitted spans surface sooner.
    // The real fix is to subscribe to the engine's reactive trace-rows feed
    // over the existing streams WebSocket instead of polling at all.
    refetchInterval: isPaused ? false : 1000,
    staleTime: 1000,
  })

  useEffect(() => {
    if (!tracesData) return

    if (tracesData.spans && tracesData.spans.length > 0) {
      // Preserve the server-provided order: the backend already sorts by the
      // requested sort_by/sort_order. Re-sorting here would override the user's
      // sort selection (e.g. Duration Asc/Desc).
      const traces = buildTraceGroups(tracesData.spans)

      const fingerprint = traceGroupsFingerprint(traces)
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
