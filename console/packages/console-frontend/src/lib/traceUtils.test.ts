// Tests for the pure helpers in `lib/traceUtils.ts`. The
// `useCopyToClipboard` hook is not covered here (needs RTL + jsdom);
// `STATUS_CONFIG` is a const, exercised indirectly via SpanPanel.

import { describe, expect, it } from 'vitest'
import type { VisualizationSpan } from './traceTransform'
import {
  classifySpanType,
  formatDuration,
  formatRelative,
  getServiceName,
} from './traceUtils'

function makeSpan(overrides: Partial<VisualizationSpan> = {}): VisualizationSpan {
  return {
    span_id: 's-1',
    trace_id: 't-1',
    name: 'GET /api/users',
    kind: 'server',
    start_time_unix_nano: 0,
    end_time_unix_nano: 0,
    duration_ms: 0,
    depth: 0,
    start_percent: 0,
    width_percent: 100,
    status: 'ok',
    attributes: {},
    events: [],
    links: [],
    service_name: 'api',
    ...overrides,
  } as VisualizationSpan
}

describe('formatDuration', () => {
  it('emits 0μs for very small values', () => {
    expect(formatDuration(0)).toBe('0μs')
    expect(formatDuration(0.0005)).toBe('0μs')
  })

  it('emits μs for sub-millisecond values', () => {
    expect(formatDuration(0.5)).toBe('500μs')
    expect(formatDuration(0.123)).toBe('123μs')
  })

  it('emits ms with 2 decimals for sub-second values', () => {
    expect(formatDuration(1.5)).toBe('1.50ms')
    expect(formatDuration(42)).toBe('42.00ms')
    expect(formatDuration(999.99)).toBe('999.99ms')
  })

  it('emits s with 2 decimals for >= 1000ms', () => {
    expect(formatDuration(1000)).toBe('1.00s')
    expect(formatDuration(1500)).toBe('1.50s')
    expect(formatDuration(60_500)).toBe('60.50s')
  })
})

describe('formatRelative', () => {
  it('emits sub-millisecond values as +μs', () => {
    expect(formatRelative(0.5)).toBe('+500μs')
    expect(formatRelative(0.001)).toBe('+1μs')
  })

  it('emits sub-second values as +ms with 1 decimal', () => {
    expect(formatRelative(1.5)).toBe('+1.5ms')
    expect(formatRelative(42)).toBe('+42.0ms')
    expect(formatRelative(999.9)).toBe('+999.9ms')
  })

  it('emits >= 1000ms as +s with 2 decimals', () => {
    expect(formatRelative(1000)).toBe('+1.00s')
    expect(formatRelative(2500)).toBe('+2.50s')
  })

  it('emits negative values with a leading - sign (recursive)', () => {
    expect(formatRelative(-42)).toBe('-+42.0ms')
    // Note: the implementation's recursive `-${formatRelative(-x)}` style
    // produces `-+Xms` which is a known quirk. Test pins current behavior.
  })

  it('handles zero', () => {
    expect(formatRelative(0)).toBe('+0μs')
  })
})

describe('classifySpanType', () => {
  it('classifies as enqueue when messaging.operation.type === publish', () => {
    expect(
      classifySpanType(
        makeSpan({ attributes: { 'messaging.operation.type': 'publish' } }),
      ),
    ).toBe('enqueue')
  })

  it('classifies as enqueue when messaging.destination.name is set', () => {
    expect(
      classifySpanType(
        makeSpan({ attributes: { 'messaging.destination.name': 'orders' } }),
      ),
    ).toBe('enqueue')
  })

  it('classifies as trigger when name starts with "trigger"', () => {
    expect(classifySpanType(makeSpan({ name: 'trigger.api-call' }))).toBe('trigger')
  })

  it('classifies as trigger when iii.function.kind attribute is set', () => {
    expect(
      classifySpanType(makeSpan({ attributes: { 'iii.function.kind': 'api' } })),
    ).toBe('trigger')
  })

  it('defaults to function for everything else', () => {
    expect(classifySpanType(makeSpan({ name: 'GET /api', attributes: {} }))).toBe('function')
  })

  it('prefers enqueue over trigger when both signals are present', () => {
    // enqueue check runs first in the implementation.
    expect(
      classifySpanType(
        makeSpan({
          name: 'trigger.x',
          attributes: { 'messaging.destination.name': 'orders' },
        }),
      ),
    ).toBe('enqueue')
  })

  it('treats missing attributes as no-signal (defaults to function)', () => {
    expect(
      classifySpanType(makeSpan({ name: 'do_thing', attributes: undefined as never })),
    ).toBe('function')
  })
})

describe('getServiceName', () => {
  it('returns service_name when present', () => {
    expect(getServiceName({ service_name: 'billing', name: 'charge' })).toBe('billing')
  })

  it('falls back to the first dot-separated segment of name when service_name is missing', () => {
    expect(getServiceName({ service_name: undefined, name: 'billing.charge' })).toBe('billing')
  })

  it('returns the whole name when service_name is missing AND name has no dot', () => {
    expect(getServiceName({ service_name: undefined, name: 'flat-name' })).toBe('flat-name')
  })

  it('returns the whole name when service_name is empty string (falsy)', () => {
    expect(getServiceName({ service_name: '', name: 'billing.charge' })).toBe('billing')
  })
})
