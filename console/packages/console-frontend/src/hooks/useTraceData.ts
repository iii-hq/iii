import { useQuery } from '@tanstack/react-query'
import { useEffect, useRef, useState } from 'react'
import { fetchTraces } from '@/api'
import { createConsoleEventsSubscription } from '@/api/consoleEvents'
import type { TracesFilterParams } from '@/api/observability/traces'
import { buildTraceGroups, type TraceGroup, traceGroupsFingerprint } from '@/lib/traceGroups'

const DEFAULT_TRACE_LIMIT = 500

export type { TraceGroup } from '@/lib/traceGroups'

export interface UseTraceDataOptions {
  filterParams: TracesFilterParams
  showSystem: boolean
  debouncedSearch: string
  isPaused: boolean
  /** Called on every trace-change tick (even while paused) with the touched
   * trace ids, so the detail view can refresh an open trace. */
  onTracesChanged?: (traceIds: string[]) => void
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
  onTracesChanged,
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
  })

  // Notify-then-query: the console worker owns a `trace` trigger on the
  // engine and forwards its coalesced `{trace_ids}` ticks over
  // /ws/console-events; each tick re-runs the filtered query above. No
  // polling — an idle engine produces zero traffic — and every (re)connect
  // resyncs with one refetch, covering ticks missed while disconnected.
  const isPausedRef = useRef(isPaused)
  isPausedRef.current = isPaused
  const refetchRef = useRef(refetch)
  refetchRef.current = refetch
  const onTracesChangedRef = useRef(onTracesChanged)
  onTracesChangedRef.current = onTracesChanged

  useEffect(() => {
    return createConsoleEventsSubscription({
      onConnect: () => refetchRef.current(),
      onTracesChanged: (traceIds) => {
        if (!isPausedRef.current) refetchRef.current()
        onTracesChangedRef.current?.(traceIds)
      },
    })
  }, [])

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
