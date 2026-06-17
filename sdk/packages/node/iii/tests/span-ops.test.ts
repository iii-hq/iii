import { describe, it, expect, beforeAll, afterEach } from 'vitest'
import { context, trace } from '@opentelemetry/api'
import {
  BasicTracerProvider,
  InMemorySpanExporter,
  SimpleSpanProcessor,
} from '@opentelemetry/sdk-trace-base'
import { SpanStatusCode } from '@opentelemetry/api'
import {
  currentSpanIsRecording,
  recordSpanEvent,
  setCurrentSpanAttribute,
  setCurrentSpanError,
} from '@iii-dev/helpers/observability'
import { AsyncLocalStorageContextManager } from '@opentelemetry/context-async-hooks'

let activeProvider: BasicTracerProvider | null = null

beforeAll(() => {
  const cm = new AsyncLocalStorageContextManager()
  cm.enable()
  context.setGlobalContextManager(cm)
})

afterEach(async () => {
  if (activeProvider) {
    await activeProvider.shutdown()
    activeProvider = null
  }
})

function buildTestProvider() {
  const exporter = new InMemorySpanExporter()
  const provider = new BasicTracerProvider({
    spanProcessors: [new SimpleSpanProcessor(exporter)],
  })
  activeProvider = provider
  const tracer = provider.getTracer('test')
  return { tracer, exporter, provider }
}

describe('recordSpanEvent', () => {
  it('writes the event with attributes to the active span', () => {
    const { tracer, exporter } = buildTestProvider()
    const span = tracer.startSpan('inner')

    context.with(trace.setSpan(context.active(), span), () => {
      recordSpanEvent('iii.invocation.input', {
        'iii.payload.json': '{"x":1}',
        'iii.payload.truncated': false,
      })
    })

    span.end()
    const spans = exporter.getFinishedSpans()
    expect(spans).toHaveLength(1)
    const event = spans[0].events.find((e) => e.name === 'iii.invocation.input')
    expect(event).toBeDefined()
    expect(event?.attributes?.['iii.payload.json']).toBe('{"x":1}')
    expect(event?.attributes?.['iii.payload.truncated']).toBe(false)
  })

  it('is a no-op when no span is active', () => {
    const { exporter } = buildTestProvider()
    recordSpanEvent('orphan', { k: 'v' })
    expect(exporter.getFinishedSpans()).toHaveLength(0)
  })
})

describe('currentSpanIsRecording', () => {
  it('returns false with no active span', () => {
    expect(currentSpanIsRecording()).toBe(false)
  })

  it('returns true inside an active recording span', () => {
    const { tracer } = buildTestProvider()
    const span = tracer.startSpan('inner')
    context.with(trace.setSpan(context.active(), span), () => {
      expect(currentSpanIsRecording()).toBe(true)
    })
    span.end()
  })
})

describe('setCurrentSpanAttribute', () => {
  it('writes the attribute to the active span', () => {
    const { tracer, exporter } = buildTestProvider()
    const span = tracer.startSpan('inner')

    context.with(trace.setSpan(context.active(), span), () => {
      setCurrentSpanAttribute('iii.session.id', 'S-1')
    })

    span.end()
    const spans = exporter.getFinishedSpans()
    expect(spans).toHaveLength(1)
    expect(spans[0].attributes['iii.session.id']).toBe('S-1')
  })

  it('is a no-op when no span is active', () => {
    const { exporter } = buildTestProvider()
    setCurrentSpanAttribute('orphan.key', 'value')
    expect(exporter.getFinishedSpans()).toHaveLength(0)
  })
})

describe('setCurrentSpanError', () => {
  it('marks the active span as Status::Error with the given message', () => {
    const { tracer, exporter } = buildTestProvider()
    const span = tracer.startSpan('inner')

    context.with(trace.setSpan(context.active(), span), () => {
      setCurrentSpanError('boom')
    })

    span.end()
    const spans = exporter.getFinishedSpans()
    expect(spans).toHaveLength(1)
    expect(spans[0].status.code).toBe(SpanStatusCode.ERROR)
    expect(spans[0].status.message).toBe('boom')
  })

  it('is a no-op when no span is active', () => {
    setCurrentSpanError('boom')
  })
})
