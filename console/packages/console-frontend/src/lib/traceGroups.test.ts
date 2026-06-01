import { describe, expect, it } from 'vitest'
import type { StoredSpan } from '@/api/observability/traces'
import { buildTraceGroups, traceGroupsFingerprint } from './traceGroups'

function span(overrides: Partial<StoredSpan> & Pick<StoredSpan, 'trace_id'>): StoredSpan {
  return {
    span_id: overrides.span_id ?? overrides.trace_id,
    name: 'op',
    start_time_unix_nano: 0,
    end_time_unix_nano: 0,
    status: 'ok',
    attributes: [],
    events: [],
    links: [],
    ...overrides,
  }
}

describe('buildTraceGroups', () => {
  // Regression for iii-hq/iii: the trace list re-sorted spans by start time on
  // the client, silently overriding the server's sort_by/sort_order. Duration
  // Asc/Desc then appeared to do nothing (or the reverse of what was selected).
  // The server already sorts; the mapper must preserve that order.
  it('preserves the server-provided order (does not re-sort by start time)', () => {
    // Server order for "Duration Asc": shortest first. Start times are chosen so
    // that re-sorting by start time descending would reorder to [s2, s3, s1].
    const spans = [
      span({ trace_id: 's1', start_time_unix_nano: 10, end_time_unix_nano: 20 }), // dur 10
      span({ trace_id: 's2', start_time_unix_nano: 300, end_time_unix_nano: 320 }), // dur 20
      span({ trace_id: 's3', start_time_unix_nano: 200, end_time_unix_nano: 230 }), // dur 30
    ]

    const groups = buildTraceGroups(spans)

    expect(groups.map((g) => g.traceId)).toEqual(['s1', 's2', 's3'])
  })

  it('computes duration and maps error status', () => {
    const [group] = buildTraceGroups([
      span({ trace_id: 't1', start_time_unix_nano: 100, end_time_unix_nano: 175, status: 'ERROR' }),
    ])

    expect(group.duration).toBe(75)
    expect(group.status).toBe('error')
  })
})

describe('traceGroupsFingerprint', () => {
  // Regression for iii-hq/iii: the old fingerprint was length + first id + last
  // id + last startTime, so a middle-only reorder ([A,B,C,D] -> [A,C,B,D]) — the
  // exact shape a sort_by change produces — fingerprinted identically and the
  // list froze on the old order. The fingerprint must reflect full ordering.
  it('changes when only the middle order changes (same first, last, length)', () => {
    const s1 = span({ trace_id: 'a', start_time_unix_nano: 100 })
    const s2 = span({ trace_id: 'b', start_time_unix_nano: 200 })
    const s3 = span({ trace_id: 'c', start_time_unix_nano: 300 })
    const s4 = span({ trace_id: 'd', start_time_unix_nano: 400 })

    const original = traceGroupsFingerprint(buildTraceGroups([s1, s2, s3, s4]))
    const reordered = traceGroupsFingerprint(buildTraceGroups([s1, s3, s2, s4]))

    expect(reordered).not.toBe(original)
  })

  it('is stable for identical input so unchanged polls are skipped', () => {
    const spans = [
      span({ trace_id: 'a', start_time_unix_nano: 100, end_time_unix_nano: 150 }),
      span({ trace_id: 'b', start_time_unix_nano: 200, end_time_unix_nano: 260 }),
    ]

    expect(traceGroupsFingerprint(buildTraceGroups(spans))).toBe(
      traceGroupsFingerprint(buildTraceGroups(spans)),
    )
  })

  it('changes when a row content changes (status or timing) at the same id', () => {
    const base = traceGroupsFingerprint(
      buildTraceGroups([
        span({ trace_id: 'a', start_time_unix_nano: 100, end_time_unix_nano: 150 }),
      ]),
    )
    const statusChanged = traceGroupsFingerprint(
      buildTraceGroups([
        span({
          trace_id: 'a',
          start_time_unix_nano: 100,
          end_time_unix_nano: 150,
          status: 'ERROR',
        }),
      ]),
    )
    const timingChanged = traceGroupsFingerprint(
      buildTraceGroups([
        span({ trace_id: 'a', start_time_unix_nano: 100, end_time_unix_nano: 999 }),
      ]),
    )

    expect(statusChanged).not.toBe(base)
    expect(timingChanged).not.toBe(base)
  })
})
