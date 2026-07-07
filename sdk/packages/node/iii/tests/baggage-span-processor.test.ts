
import type { AttributeValue } from '@opentelemetry/api'
import { context, propagation, ROOT_CONTEXT } from '@opentelemetry/api'
import {
  AlwaysOffSampler,
  BasicTracerProvider,
  InMemorySpanExporter,
  SimpleSpanProcessor,
} from '@opentelemetry/sdk-trace-base'
import { describe, expect, it } from 'vitest'

import { BaggageSpanProcessor } from '@iii-dev/helpers/observability'

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
  it('copies every baggage entry to attributes', () => {
    // No key policy: which attributes *mean* something (`iii.tag.*`,
    // `iii.session.*`) is a query-side convention owned by the engine's
    // traces API — a filtering policy baked into worker binaries would
    // silently drop newer keys from any worker running an older SDK.
    const { tracer, exporter } = buildTestProvider(new BaggageSpanProcessor())

    const entries = {
      'iii.session.id': 'S-1',
      'iii.session.name': 'refactor auth',
      'iii.message.id': 'M-1',
      'iii.function.id': 'auth::set_token',
      'iii.tag.message': 'fix the login bug',
      // Non-iii baggage is a first-class tag source too.
      'tenant.id': 't-42',
    }
    withBaggage(entries, () => {
      const span = tracer.startSpan('inner')
      span.end()
    })

    for (const [key, value] of Object.entries(entries)) {
      expect(firstSpanAttr(exporter, key), key).toBe(value)
    }
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
})
