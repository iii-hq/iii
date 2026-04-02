/**
 * Tests verifying that recordException captures exception.stacktrace
 * when a span records an error.
 *
 * Uses the OTel API directly with an InMemorySpanExporter to verify
 * span event attributes without needing a real engine connection.
 */

import { describe, it, expect, afterEach, vi } from 'vitest'

// Mock WebSocket to prevent real connections from the global setup
vi.mock('ws', () => {
  const MockWebSocket = vi.fn().mockImplementation(() => ({
    on: vi.fn(),
    close: vi.fn(),
    send: vi.fn(),
    readyState: 0,
  }))
  return { WebSocket: MockWebSocket, default: { WebSocket: MockWebSocket } }
})

import { SpanStatusCode } from '@opentelemetry/api'
import {
  BasicTracerProvider,
  InMemorySpanExporter,
  SimpleSpanProcessor,
} from '@opentelemetry/sdk-trace-base'

describe('OTel exception recording', () => {
  let provider: BasicTracerProvider
  let exporter: InMemorySpanExporter

  afterEach(async () => {
    if (provider) {
      await provider.shutdown()
    }
  })

  it('recordException captures exception.stacktrace on error', async () => {
    exporter = new InMemorySpanExporter()
    provider = new BasicTracerProvider()
    provider.addSpanProcessor(new SimpleSpanProcessor(exporter))

    const tracer = provider.getTracer('test-tracer')

    const span = tracer.startSpan('test-span')
    try {
      throw new Error('test error for stacktrace')
    } catch (error) {
      span.setStatus({ code: SpanStatusCode.ERROR, message: (error as Error).message })
      span.recordException(error as Error)
    } finally {
      span.end()
    }

    await provider.forceFlush()

    const spans = exporter.getFinishedSpans()
    expect(spans.length).toBeGreaterThanOrEqual(1)

    const finishedSpan = spans[0]
    const exceptionEvent = finishedSpan.events.find((e) => e.name === 'exception')

    expect(exceptionEvent).toBeDefined()
    expect(exceptionEvent!.attributes).toBeDefined()
    expect(exceptionEvent!.attributes!['exception.type']).toBe('Error')
    expect(exceptionEvent!.attributes!['exception.message']).toBe('test error for stacktrace')
    expect(exceptionEvent!.attributes!['exception.stacktrace']).toBeDefined()
    expect(String(exceptionEvent!.attributes!['exception.stacktrace'])).toContain(
      'Error: test error for stacktrace',
    )
  })

  it('successful span has no exception event', async () => {
    exporter = new InMemorySpanExporter()
    provider = new BasicTracerProvider()
    provider.addSpanProcessor(new SimpleSpanProcessor(exporter))

    const tracer = provider.getTracer('test-tracer')

    const span = tracer.startSpan('success-span')
    span.setStatus({ code: SpanStatusCode.OK })
    span.end()

    await provider.forceFlush()

    const spans = exporter.getFinishedSpans()
    expect(spans.length).toBeGreaterThanOrEqual(1)

    const finishedSpan = spans[0]
    const exceptionEvent = finishedSpan.events.find((e) => e.name === 'exception')
    expect(exceptionEvent).toBeUndefined()
  })
})
