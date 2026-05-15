// Pure helpers for the TRACES tab "Group by" affordance.
//
// The server-side aggregation (`engine::traces::group_by`) returns
// `TraceGroup[]`; this module formats and classifies that data for the
// UI without re-implementing any grouping logic. Extracted from the
// route component so it's testable in isolation when the console grows
// test infrastructure.

import type { TraceGroup } from '../api/observability/traces'

/**
 * Span attributes the TRACES tab can group by. Aligned with the
 * iii-sdk allowlist (`iii.session.id`, `iii.message.id`,
 * `iii.function.id`). Keep in lock-step.
 */
export type GroupByAttribute = 'iii.message.id' | 'iii.session.id' | 'iii.function.id'

export type GroupByOption = 'none' | GroupByAttribute

/**
 * Human-readable label for the dropdown.
 */
export function groupByLabel(option: GroupByOption): string {
  switch (option) {
    case 'none':
      return 'No grouping'
    case 'iii.message.id':
      return 'Group by message'
    case 'iii.session.id':
      return 'Group by session'
    case 'iii.function.id':
      return 'Group by function'
  }
}

/**
 * Format the group's metadata into the inline summary string the row
 * shows, e.g. "(12 spans, 1.42s, 1 error)".
 *
 * Handles the wildcard session sentinel `*` from `ui::subscribe` /
 * `ui::unsubscribe` calls by emitting "all sessions (wildcard)" rather
 * than the literal asterisk.
 */
export function summarizeGroup(group: TraceGroup): string {
  const errors =
    group.error_count === 0
      ? '0 errors'
      : group.error_count === 1
        ? '1 error'
        : `${group.error_count} errors`
  return `${group.span_count} spans, ${formatDurationMs(group.duration_ms)}, ${errors}`
}

/**
 * Display string for the group's heading. Hides the wildcard sentinel
 * behind a friendlier label.
 */
export function groupHeading(group: TraceGroup, option: GroupByAttribute): string {
  if (group.value === '*' && option === 'iii.session.id') {
    return 'all sessions (wildcard)'
  }
  return group.value
}

function formatDurationMs(ms: number): string {
  if (ms < 1) return '<1ms'
  if (ms < 1_000) return `${Math.round(ms)}ms`
  if (ms < 60_000) return `${(ms / 1_000).toFixed(2)}s`
  const seconds = Math.round(ms / 1_000)
  const m = Math.floor(seconds / 60)
  const s = seconds % 60
  return `${m}m${s}s`
}

/**
 * Heuristic: tells the caller whether `engine::traces::group_by` is
 * unavailable in this engine deploy (older version, or
 * `iii-observability` not configured). Used so the dropdown can hide
 * itself gracefully rather than surfacing an error.
 *
 * The engine returns `function_not_found` as the error_code; the fetch
 * path surfaces it as a thrown Error whose message contains the string.
 * String-match is intentionally generous — different runtimes wrap the
 * error differently.
 */
export function isGroupByUnavailable(err: unknown): boolean {
  const message = err instanceof Error ? err.message : String(err)
  return /function[_ ]not[_ ]found|group_by|404/.test(message.toLowerCase())
}
