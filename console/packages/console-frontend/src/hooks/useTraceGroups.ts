import { useQuery } from '@tanstack/react-query'
import { fetchTracesGroupBy } from '@/api'
import { useEngineSdk } from '@/api/engine-sdk-provider'
import type { TraceGroup, TracesGroupByResponse } from '@/api/observability/traces'
import type { GroupByOption } from '@/lib/groupTraces'
import { isGroupByUnavailable } from '@/lib/groupTraces'

const DEFAULT_GROUP_LIMIT = 100

export interface UseTraceGroupsOptions {
  /** When 'none', the hook is disabled (no network calls). */
  groupBy: GroupByOption
  /** Include engine-internal spans. */
  includeInternal: boolean
  /** When true, suspend auto-refresh (matches the flat-list hook). */
  isPaused: boolean
}

export interface UseTraceGroupsReturn {
  groups: TraceGroup[]
  isLoading: boolean
  /**
   * Set when the engine doesn't expose `engine::traces::group_by` (older
   * deploy or `iii-observability` not running). UI should hide the group
   * view and fall back to the flat list when this is true.
   */
  unavailable: boolean
  refetch: () => void
}

/**
 * React Query wrapper around `fetchTracesGroupBy`. Returns the
 * server-aggregated group list and surfaces a soft-failure flag when
 * the endpoint isn't available so callers can degrade gracefully
 * rather than showing an error pill.
 */
export function useTraceGroups({
  groupBy,
  includeInternal,
  isPaused,
}: UseTraceGroupsOptions): UseTraceGroupsReturn {
  const sdk = useEngineSdk()
  const enabled = groupBy !== 'none'

  const { data, isLoading, error, refetch } = useQuery<TracesGroupByResponse, Error>({
    queryKey: ['traceGroups', groupBy, includeInternal],
    enabled,
    queryFn: () =>
      fetchTracesGroupBy(sdk, {
        attribute: groupBy,
        limit: DEFAULT_GROUP_LIMIT,
        include_internal: includeInternal,
      }),
    refetchInterval: isPaused ? false : 3000,
    staleTime: 1000,
    retry: (failureCount, err) => {
      // Don't retry when the endpoint is missing — the UI hides the
      // affordance in that case and waits for the user to switch back
      // to flat-list mode.
      if (isGroupByUnavailable(err)) return false
      return failureCount < 2
    },
  })

  return {
    groups: data?.groups ?? [],
    isLoading,
    unavailable: !!error && isGroupByUnavailable(error),
    refetch,
  }
}
