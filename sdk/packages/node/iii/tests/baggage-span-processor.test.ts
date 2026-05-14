// Unit tests for BaggageSpanProcessor. Mirrors the Rust impl tests at
// motia/sdk/packages/rust/iii/src/telemetry/baggage_span_processor.rs::tests
// so cross-language behavior stays in lock-step.

import type { AttributeValue } from '@opentelemetry/api'
import { context, propagation, ROOT_CONTEXT } from '@opentelemetry/api'
import {
  AlwaysOffSampler,
  BasicTracerProvider,
  InMemorySpanExporter,
  SimpleSpanProcessor,
} from '@opentelemetry/sdk-trace-base'
import { describe, expect, it } from 'vitest'

import { BaggageSpanProcessor, DEFAULT_ALLOWLIST } from '../src/telemetry-system/baggage-span-processor'

/**
 * Build a tracer provider with BaggageSpanProcessor chained before an
 * InMemorySpanExporter. Exactly the layering `initOtel` installs.
 */
function buildTestProvider(processor: BaggageSpanProcessor) {
  const exporter = new InMemorySpanExporter()
  const provider = new BasicTracerProvider({
    spanProcessors: [processor, new SimpleSpanProcessor(exporter)],
  })
  const tracer = provider.getTracer('test')
  return { tracer, exporter, provider }
}

/** Run `fn` with a context that has the given baggage entries set. */
function withBaggage<T>(entries: Record<string, string>, fn: () => T): T {
  let bag = propagation.createBaggage()
  for (const [k, v] of Object.entries(entries)) {
    bag = bag.setEntry(k, { value: v })
  }
  return context.with(propagation.setBaggage(ROOT_CONTEXT, bag), fn)
}

function firstSpanAttr(exporter: InMemorySpanExporter, key: string): AttributeValue | undefined {
  const spans = exporter.getFinishedSpans()
  return spans[0]?.attributes[key]
}

describe('BaggageSpanProcessor', () => {
  it('copies default allowlist from baggage to attributes', () => {
    const { tracer, exporter } = buildTestProvider(new BaggageSpanProcessor())

    withBaggage(
      {
        'iii.session.id': 'S-1',
        'iii.message.id': 'M-1',
        'iii.function_id': 'auth::set_token',
      },
      () => {
        const span = tracer.startSpan('inner')
        span.end()
      },
    )

    expect(firstSpanAttr(exporter, 'iii.session.id')).toBe('S-1')
    expect(firstSpanAttr(exporter, 'iii.message.id')).toBe('M-1')
    expect(firstSpanAttr(exporter, 'iii.function_id')).toBe('auth::set_token')
  })

  it('missing baggage entry means attribute not set', () => {
    const { tracer, exporter } = buildTestProvider(new BaggageSpanProcessor())

    withBaggage({ 'iii.message.id': 'M-only' }, () => {
      const span = tracer.startSpan('inner')
      span.end()
    })

    expect(firstSpanAttr(exporter, 'iii.message.id')).toBe('M-only')
    expect(firstSpanAttr(exporter, 'iii.session.id')).toBeUndefined()
    expect(firstSpanAttr(exporter, 'iii.function_id')).toBeUndefined()
  })

  it('baggage entries not in allowlist are dropped', () => {
    const { tracer, exporter } = buildTestProvider(new BaggageSpanProcessor())

    withBaggage(
      {
        'iii.message.id': 'M',
        'tenant.id': 't-42',
        'debug.feature_flag': 'on',
      },
      () => {
        const span = tracer.startSpan('inner')
        span.end()
      },
    )

    expect(firstSpanAttr(exporter, 'iii.message.id')).toBe('M')
    expect(firstSpanAttr(exporter, 'tenant.id')).toBeUndefined()
    expect(firstSpanAttr(exporter, 'debug.feature_flag')).toBeUndefined()
  })

  it('custom allowlist is honored', () => {
    const { tracer, exporter } = buildTestProvider(
      new BaggageSpanProcessor(['tenant.id', 'iii.message.id']),
    )

    withBaggage(
      {
        'tenant.id': 't-1',
        'iii.message.id': 'M',
        'iii.session.id': 'S-not-copied',
      },
      () => {
        const span = tracer.startSpan('inner')
        span.end()
      },
    )

    expect(firstSpanAttr(exporter, 'tenant.id')).toBe('t-1')
    expect(firstSpanAttr(exporter, 'iii.message.id')).toBe('M')
    // Default allowlist key, dropped because not in custom allowlist:
    expect(firstSpanAttr(exporter, 'iii.session.id')).toBeUndefined()
  })

  it('empty parent context produces no attributes', () => {
    const { tracer, exporter } = buildTestProvider(new BaggageSpanProcessor())

    const span = tracer.startSpan('inner')
    span.end()

    expect(firstSpanAttr(exporter, 'iii.session.id')).toBeUndefined()
    expect(firstSpanAttr(exporter, 'iii.message.id')).toBeUndefined()
  })

  it('NoOp guard skips processing when sampled out', () => {
    // With AlwaysOffSampler, the span returned by the tracer is not
    // recording. The `onStart` guard must short-circuit BEFORE calling
    // `propagation.getBaggage` — both as a panic safeguard and as the
    // documented allocation-skipping perf optimization. Pins the guard
    // so a regression that removes it is caught.
    const exporter = new InMemorySpanExporter()
    const provider = new BasicTracerProvider({
      sampler: new AlwaysOffSampler(),
      spanProcessors: [new BaggageSpanProcessor(), new SimpleSpanProcessor(exporter)],
    })
    const tracer = provider.getTracer('test')

    withBaggage(
      {
        'iii.session.id': 'S-1',
        'iii.message.id': 'M-1',
      },
      () => {
        const span = tracer.startSpan('inner')
        span.end()
      },
    )

    expect(exporter.getFinishedSpans()).toHaveLength(0)
  })

  it('default allowlist matches the Rust SDK and harness contract', () => {
    // Lock-step contract. Single source of truth lives in
    // docs/superpowers/specs/2026-05-12-...md §3 step 2 and the Rust
    // harness HARNESS_KEYS constant. If a new harness baggage key lands,
    // the Rust-side cross-crate test catches it on the Rust side; this
    // test catches drift on the Node side.
    expect([...DEFAULT_ALLOWLIST]).toEqual([
      'iii.session.id',
      'iii.message.id',
      'iii.function_id',
    ])
  })
})
