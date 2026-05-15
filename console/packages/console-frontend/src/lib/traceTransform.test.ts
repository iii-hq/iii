// Tests for the pure timestamp helpers in `lib/traceTransform.ts`.
// `toWaterfallData` and `treeToWaterfallData` are higher-level
// transforms with many derived fields (depth, percent positioning);
// they're indirectly exercised by the rest of the test suite and
// would benefit from richer fixture coverage in a follow-up.

import { describe, expect, it } from 'vitest'
import { calculateDurationMs, toMs } from './traceTransform'

const NS_PER_MS = 1_000_000
// NANO_THRESHOLD in the module is Jan 1 2100 in ms (4102444800000).
// Any number above that is treated as nanoseconds.
const NANO_THRESHOLD = 4_102_444_800_000

describe('toMs', () => {
  it('passes ms-magnitude values through unchanged', () => {
    expect(toMs(0)).toBe(0)
    expect(toMs(1_700_000_000_000)).toBe(1_700_000_000_000)
  })

  it('divides ns-magnitude values by 1_000_000', () => {
    // A 2024-era nanosecond timestamp is ~1.7e18; toMs should return ~1.7e12 (ms).
    const ns = 1_700_000_000_000 * NS_PER_MS
    expect(toMs(ns)).toBe(1_700_000_000_000)
  })

  it('uses the exact NANO_THRESHOLD boundary', () => {
    // Values equal to the threshold are treated as ms (`>` not `>=`).
    expect(toMs(NANO_THRESHOLD)).toBe(NANO_THRESHOLD)
    // Just above is treated as ns and divided.
    expect(toMs(NANO_THRESHOLD + 1)).toBe((NANO_THRESHOLD + 1) / NS_PER_MS)
  })

  it('returns 0 for non-finite inputs', () => {
    expect(toMs(Number.NaN)).toBe(0)
    expect(toMs(Number.POSITIVE_INFINITY)).toBe(0)
    expect(toMs(Number.NEGATIVE_INFINITY)).toBe(0)
  })
})

describe('calculateDurationMs', () => {
  it('returns difference for ms-magnitude inputs', () => {
    expect(calculateDurationMs(100, 250)).toBe(150)
  })

  it('returns difference for ns-magnitude inputs (within float64 precision)', () => {
    // Float64 has ~15-16 decimal digits of precision; 1.7e18 + 2.5e8 ns
    // loses the last few digits on the divide-by-1e6 path. Tolerate
    // sub-millisecond error.
    const t0 = 1_700_000_000_000 * NS_PER_MS
    const t1 = t0 + 250 * NS_PER_MS
    expect(calculateDurationMs(t0, t1)).toBeCloseTo(250, 1)
  })

  it('handles mixed-magnitude inputs (one ns, one ms)', () => {
    // A bug magnet: if a tree stores start in ns and end in ms, the
    // helper should still produce a sensible result via per-arg toMs.
    const startNs = 1_700_000_000_000 * NS_PER_MS
    const endMs = 1_700_000_000_500
    expect(calculateDurationMs(startNs, endMs)).toBe(500)
  })

  it('returns 0 for negative duration (end before start)', () => {
    expect(calculateDurationMs(500, 100)).toBe(0)
  })

  it('coerces non-finite inputs to 0 via toMs (does NOT short-circuit to 0)', () => {
    // toMs(NaN) returns 0 and the function then computes the diff
    // against the other arg, so the result is the other arg's ms value
    // rather than 0. Pins this quirk: callers should validate input
    // before calling rather than relying on a sentinel.
    expect(calculateDurationMs(Number.NaN, 100)).toBe(100)
    expect(calculateDurationMs(100, Number.NaN)).toBe(0) // end < start -> guard returns 0
  })

  it('returns 0 when start equals end', () => {
    expect(calculateDurationMs(100, 100)).toBe(0)
  })
})
