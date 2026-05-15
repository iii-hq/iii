
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

function buildTestProvider(processor: BaggageSpanProcessor) {
  const exporter = new InMemorySpanExporter()
  const provider = new BasicTracerProvider({
    spanProcessors: [processor, new SimpleSpanProcessor(exporter)],
  })
  const tracer = provider.getTracer('test')
  return { tracer, exporter, provider }
}

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
        'iii.function.id': 'auth::set_token',
      },
      () => {
        const span = tracer.startSpan('inner')
        span.end()
      },
    )

    expect(firstSpanAttr(exporter, 'iii.session.id')).toBe('S-1')
    expect(firstSpanAttr(exporter, 'iii.message.id')).toBe('M-1')
    expect(firstSpanAttr(exporter, 'iii.function.id')).toBe('auth::set_token')
  })

  it('missing baggage entry means attribute not set', () => {
    const { tracer, exporter } = buildTestProvider(new BaggageSpanProcessor())

    withBaggage({ 'iii.message.id': 'M-only' }, () => {
      const span = tracer.startSpan('inner')
      span.end()
    })

    expect(firstSpanAttr(exporter, 'iii.message.id')).toBe('M-only')
    expect(firstSpanAttr(exporter, 'iii.session.id')).toBeUndefined()
    expect(firstSpanAttr(exporter, 'iii.function.id')).toBeUndefined()
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
    // DEFAULT_ALLOWLIST drift across languages would break worker chains.
    expect([...DEFAULT_ALLOWLIST]).toEqual([
      'iii.session.id',
      'iii.message.id',
      'iii.function.id',
    ])
  })
})
