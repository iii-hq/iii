import type { StoredSpan } from '@/api/observability/traces'
import { toMs } from './traceTransform'

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

/**
 * Map raw spans to display rows, preserving the server-provided order.
 *
 * The order matters: the backend already sorts by the requested
 * `sort_by`/`sort_order`. Re-sorting here would silently override the user's
 * sort selection (e.g. Duration Asc/Desc), so callers must keep this order.
 */
export function buildTraceGroups(spans: StoredSpan[]): TraceGroup[] {
  return spans.map((span) => {
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
}

/**
 * Fingerprint a list of trace rows for change detection.
 *
 * Reflects order AND per-row content so a re-sort (e.g. switching sort_by) or
 * an in-place update (status/timing change) is detected. A coarse fingerprint
 * (length + first/last id) silently misses middle-only reorders like
 * `[A,B,C,D] -> [A,C,B,D]`, which would freeze the list on a sort change.
 */
export function traceGroupsFingerprint(groups: TraceGroup[]): string {
  return groups.map((g) => `${g.traceId}:${g.status}:${g.startTime}:${g.endTime ?? ''}`).join('|')
}
