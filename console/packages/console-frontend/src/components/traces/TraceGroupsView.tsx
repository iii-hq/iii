// Renders the server-aggregated group-by view for the TRACES tab.
//
// Used when `filters.groupBy !== 'none'`. The route swaps this in place of
// the flat WaterfallChart list. Each row collapses one attribute-value's
// worth of spans across multiple traces and shows summary metadata
// (span_count, duration, error_count). Clicking a group row drills down
// to a single trace via the existing trace-detail panel; for v1, we
// surface the first trace_id in the group.
//
// If the engine doesn't expose `engine::traces::group_by` (older deploy
// or `iii-observability` not running), the `unavailable` flag from the
// hook surfaces a soft fallback message — the user can switch back to
// "No grouping" without seeing an error toast.

import { Layers, Loader2 } from 'lucide-react'

import type { TraceGroup } from '@/api/observability/traces'
import { useTraceGroups } from '@/hooks/useTraceGroups'
import type { GroupByAttribute } from '@/lib/groupTraces'
import { groupHeading, summarizeGroup } from '@/lib/groupTraces'

interface TraceGroupsViewProps {
  attribute: GroupByAttribute
  showSystem: boolean
  isPaused: boolean
  onSelectTrace: (traceId: string) => void
  /**
   * Called with the full TraceGroup when a row is clicked. Routes that
   * want a session-style multi-trace detail panel use this; older
   * call-sites can ignore it and rely solely on `onSelectTrace`.
   */
  onSelectGroup?: (group: TraceGroup) => void
  selectedTraceId: string | null
}

export function TraceGroupsView({
  attribute,
  showSystem,
  isPaused,
  onSelectTrace,
  onSelectGroup,
  selectedTraceId,
}: TraceGroupsViewProps) {
  const { groups, isLoading, unavailable } = useTraceGroups({
    groupBy: attribute,
    includeInternal: showSystem,
    isPaused,
  })

  if (unavailable) {
    return (
      <div className="flex flex-col items-center justify-center py-12 px-6 text-center text-[12px] text-muted">
        <Layers className="w-6 h-6 mb-2 opacity-50" />
        <div className="font-medium text-foreground mb-1">Group-by not available</div>
        <div className="max-w-md">
          The engine doesn't expose <code className="text-yellow">engine::traces::group_by</code>.
          Either the engine is older than the version that introduced it, or the
          <code className="text-yellow"> iii-observability</code> worker is not configured.
          Switch &quot;Group by&quot; back to &quot;No grouping&quot; to use the flat trace list.
        </div>
      </div>
    )
  }

  if (isLoading && groups.length === 0) {
    return (
      <div className="flex items-center justify-center py-12 text-[12px] text-muted gap-2">
        <Loader2 className="w-4 h-4 animate-spin" />
        Loading groups…
      </div>
    )
  }

  if (groups.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center py-12 px-6 text-center text-[12px] text-muted">
        <Layers className="w-6 h-6 mb-2 opacity-50" />
        <div>No spans match this attribute yet.</div>
      </div>
    )
  }

  return (
    <div className="flex flex-col">
      {groups.map((group) => (
        <GroupRow
          key={`${attribute}:${group.value}`}
          attribute={attribute}
          group={group}
          isSelected={selectedTraceId !== null && group.trace_ids.includes(selectedTraceId)}
          onClick={() => {
            // Surface the full group when a session-aware container is
            // wired in (routes/traces.tsx). For row-highlight purposes
            // we also push the first trace_id into the legacy
            // single-trace selection so isSelected styling still works.
            const firstTrace = group.trace_ids[0]
            if (firstTrace) onSelectTrace(firstTrace)
            if (onSelectGroup) onSelectGroup(group)
          }}
        />
      ))}
    </div>
  )
}

interface GroupRowProps {
  attribute: GroupByAttribute
  group: TraceGroup
  isSelected: boolean
  onClick: () => void
}

function GroupRow({ attribute, group, isSelected, onClick }: GroupRowProps) {
  const heading = groupHeading(group, attribute)
  const summary = summarizeGroup(group)
  return (
    <button
      type="button"
      onClick={onClick}
      className={`flex items-center gap-2 px-3 py-2 text-left border-b border-[#1D1D1D]/60 transition-colors ${
        isSelected ? 'bg-[#F3F724]/[0.06] border-l-2 border-l-[#F3F724]' : 'hover:bg-[#1D1D1D]/50'
      }`}
      title={`${heading} (${group.trace_ids.length} trace${group.trace_ids.length === 1 ? '' : 's'})`}
    >
      <Layers
        className={`w-3.5 h-3.5 flex-shrink-0 ${
          group.error_count > 0 ? 'text-red-400' : 'text-muted'
        }`}
      />
      <span
        className={`font-mono text-[12px] truncate ${
          isSelected ? 'text-[#F3F724]' : 'text-foreground'
        }`}
      >
        {heading}
      </span>
      <span className="ml-auto text-[11px] text-muted flex-shrink-0">{summary}</span>
    </button>
  )
}
