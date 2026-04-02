/**
 * Unit tests for global fetch auto-instrumentation behavior.
 *
 * These tests verify that globalThis.fetch is patched by default
 * and can be disabled via fetchInstrumentationEnabled config.
 */

import { describe, it, expect, afterEach, vi, beforeEach } from 'vitest'
import { type Tracer, SpanStatusCode } from '@opentelemetry/api'
import type { Instrumentation } from '@opentelemetry/instrumentation'

// Mock WebSocket to prevent real connections
vi.mock('ws', () => {
  const MockWebSocket = vi.fn().mockImplementation(() => ({
    on: vi.fn(),
    close: vi.fn(),
    send: vi.fn(),
    readyState: 0,
  }))
  return { WebSocket: MockWebSocket, default: { WebSocket: MockWebSocket } }
})

// Mock NodeTracerProvider to avoid real tracer setup
vi.mock('@opentelemetry/sdk-trace-node', () => ({
  NodeTracerProvider: vi.fn().mockImplementation(() => ({
    register: vi.fn(),
    shutdown: vi.fn().mockResolvedValue(undefined),
  })),
}))

// Mock MeterProvider
vi.mock('@opentelemetry/sdk-metrics', () => ({
  MeterProvider: vi.fn().mockImplementation(() => ({
    getMeter: vi.fn().mockReturnValue({ createCounter: vi.fn() }),
    shutdown: vi.fn().mockResolvedValue(undefined),
  })),
  PeriodicExportingMetricReader: vi.fn().mockImplementation(() => ({})),
}))

// Mock LoggerProvider
vi.mock('@opentelemetry/sdk-logs', () => ({
  LoggerProvider: vi.fn().mockImplementation(() => ({
    getLogger: vi.fn().mockReturnValue({ emit: vi.fn() }),
    addLogRecordProcessor: vi.fn(),
    shutdown: vi.fn().mockResolvedValue(undefined),
  })),
  SimpleLogRecordProcessor: vi.fn().mockImplementation(() => ({})),
}))

// Mock span processor
vi.mock('@opentelemetry/sdk-trace-base', () => ({
  BatchSpanProcessor: vi.fn().mockImplementation(() => ({})),
}))

// Mock exporters
vi.mock('../src/telemetry-system/exporters', () => ({
  EngineSpanExporter: vi.fn().mockImplementation(() => ({})),
  EngineMetricsExporter: vi.fn().mockImplementation(() => ({})),
  EngineLogExporter: vi.fn().mockImplementation(() => ({})),
}))

// Mock the shared connection
vi.mock('../src/telemetry-system/connection', () => ({
  SharedEngineConnection: vi.fn().mockImplementation(() => ({
    send: vi.fn(),
    onConnected: vi.fn(),
    getState: vi.fn().mockReturnValue('disconnected'),
    shutdown: vi.fn().mockResolvedValue(undefined),
  })),
}))

import { initOtel, shutdownOtel, getTracer } from '../src/telemetry-system/index'

describe('Fetch instrumentation', () => {
  const originalEnv = process.env
  const nativeFetch = globalThis.fetch

  beforeEach(() => {
    process.env = { ...originalEnv }
    delete process.env.OTEL_ENABLED
    // Ensure globalThis.fetch is the native version before each test
    globalThis.fetch = nativeFetch
  })

  afterEach(async () => {
    await shutdownOtel()
    process.env = originalEnv
    // Restore native fetch after shutdown
    globalThis.fetch = nativeFetch
  })

  it('should initialize OTel successfully with fetch instrumentation enabled by default', () => {
    expect(() => initOtel()).not.toThrow()
    expect(getTracer()).not.toBeNull()
  })

  it('should patch globalThis.fetch when enabled by default', () => {
    initOtel()
    // After initOtel, globalThis.fetch should be patched (different from native)
    expect(globalThis.fetch).not.toBe(nativeFetch)
  })

  it('should NOT patch globalThis.fetch when fetchInstrumentationEnabled is false', () => {
    initOtel({ fetchInstrumentationEnabled: false })
    // globalThis.fetch should remain the native version
    expect(globalThis.fetch).toBe(nativeFetch)
  })

  it('should restore globalThis.fetch on shutdown', async () => {
    initOtel()
    expect(globalThis.fetch).not.toBe(nativeFetch)

    await shutdownOtel()
    expect(globalThis.fetch).toBe(nativeFetch)
  })

  it('should NOT patch globalThis.fetch when OTel is disabled', () => {
    initOtel({ enabled: false })
    expect(globalThis.fetch).toBe(nativeFetch)
    expect(getTracer()).toBeNull()
  })

  it('should initialize OTel successfully when fetchInstrumentationEnabled is explicitly true', () => {
    expect(() => initOtel({ fetchInstrumentationEnabled: true })).not.toThrow()
    expect(getTracer()).not.toBeNull()
    expect(globalThis.fetch).not.toBe(nativeFetch)
  })

  it('should accept user instrumentations alongside fetch patch', () => {
    const userInstrumentation = {
      instrumentationName: 'custom-test',
      instrumentationVersion: '1.0.0',
      getConfig: () => ({}),
      setConfig: vi.fn(),
      setTracerProvider: vi.fn(),
      setMeterProvider: vi.fn(),
      enable: vi.fn(),
      disable: vi.fn(),
    }
    expect(() =>
      initOtel({ instrumentations: [userInstrumentation as Instrumentation] }),
    ).not.toThrow()
    expect(getTracer()).not.toBeNull()
    // Both fetch patch and user instrumentations should work
    expect(globalThis.fetch).not.toBe(nativeFetch)
  })

  it('should accept user instrumentations when fetch instrumentation is disabled', () => {
    const userInstrumentation = {
      instrumentationName: 'custom-test',
      instrumentationVersion: '1.0.0',
      getConfig: () => ({}),
      setConfig: vi.fn(),
      setTracerProvider: vi.fn(),
      setMeterProvider: vi.fn(),
      enable: vi.fn(),
      disable: vi.fn(),
    }
    expect(() =>
      initOtel({
        fetchInstrumentationEnabled: false,
        instrumentations: [userInstrumentation as Instrumentation],
      }),
    ).not.toThrow()
    expect(getTracer()).not.toBeNull()
    // Fetch should NOT be patched
    expect(globalThis.fetch).toBe(nativeFetch)
  })
})

describe('Fetch span attributes', () => {
  const nativeFetch = globalThis.fetch
  let unpatch: () => void

  afterEach(() => {
    unpatch?.()
    // restore in case the test replaced fetch manually
    globalThis.fetch = nativeFetch
  })

  it('sets url.scheme, url.path, network.protocol.name, server.address and span name on success', async () => {
    const spanMock = {
      setAttribute: vi.fn(),
      setStatus: vi.fn(),
      recordException: vi.fn(),
      end: vi.fn(),
    }
    const tracerMock = {
      startActiveSpan: vi
        .fn()
        .mockImplementation(
          (_name: string, _opts: unknown, _ctx: unknown, fn: (span: typeof spanMock) => unknown) =>
            fn(spanMock),
        ),
    }

    const { patchGlobalFetch, unpatchGlobalFetch } = await import(
      '../src/telemetry-system/fetch-instrumentation'
    )
    unpatch = unpatchGlobalFetch

    // Replace native fetch with a fake before patching so the patch wraps the fake
    const fakeFetch = vi.fn().mockResolvedValue(new Response('ok', { status: 200 }))
    globalThis.fetch = fakeFetch

    patchGlobalFetch(tracerMock as unknown as Tracer)

    await globalThis.fetch('https://example.com/api/items?q=1')

    expect(tracerMock.startActiveSpan).toHaveBeenCalledWith(
      'GET /api/items',
      expect.objectContaining({
        attributes: expect.objectContaining({
          'http.request.method': 'GET',
          'url.full': 'https://example.com/api/items?q=1',
          'url.scheme': 'https',
          'url.path': '/api/items',
          'network.protocol.name': 'http',
          'server.address': 'example.com',
        }),
      }),
      expect.anything(),
      expect.any(Function),
    )

    expect(spanMock.setAttribute).toHaveBeenCalledWith('http.response.status_code', 200)
    expect(spanMock.setStatus).toHaveBeenCalledWith({ code: SpanStatusCode.OK })
  })

  it('sets url.query when query string is present', async () => {
    const spanMock = {
      setAttribute: vi.fn(),
      setStatus: vi.fn(),
      recordException: vi.fn(),
      end: vi.fn(),
    }
    const tracerMock = {
      startActiveSpan: vi
        .fn()
        .mockImplementation(
          (_name: string, _opts: unknown, _ctx: unknown, fn: (span: typeof spanMock) => unknown) =>
            fn(spanMock),
        ),
    }

    const { patchGlobalFetch, unpatchGlobalFetch } = await import(
      '../src/telemetry-system/fetch-instrumentation'
    )
    unpatch = unpatchGlobalFetch

    const fakeFetch = vi.fn().mockResolvedValue(new Response('ok', { status: 200 }))
    globalThis.fetch = fakeFetch

    patchGlobalFetch(tracerMock as unknown as Tracer)

    await globalThis.fetch('https://example.com/api/items?q=1&page=2')

    expect(tracerMock.startActiveSpan).toHaveBeenCalledWith(
      'GET /api/items',
      expect.objectContaining({
        attributes: expect.objectContaining({
          'url.query': 'q=1&page=2',
        }),
      }),
      expect.anything(),
      expect.any(Function),
    )
  })

  it('does not set url.query when no query string', async () => {
    const spanMock = {
      setAttribute: vi.fn(),
      setStatus: vi.fn(),
      recordException: vi.fn(),
      end: vi.fn(),
    }
    const tracerMock = {
      startActiveSpan: vi
        .fn()
        .mockImplementation(
          (_name: string, _opts: unknown, _ctx: unknown, fn: (span: typeof spanMock) => unknown) =>
            fn(spanMock),
        ),
    }

    const { patchGlobalFetch, unpatchGlobalFetch } = await import(
      '../src/telemetry-system/fetch-instrumentation'
    )
    unpatch = unpatchGlobalFetch

    const fakeFetch = vi.fn().mockResolvedValue(new Response('ok', { status: 200 }))
    globalThis.fetch = fakeFetch

    patchGlobalFetch(tracerMock as unknown as Tracer)

    await globalThis.fetch('https://example.com/api/items')

    const callArgs = tracerMock.startActiveSpan.mock.calls[0][1] as {
      attributes: Record<string, unknown>
    }
    expect(callArgs.attributes['url.query']).toBeUndefined()
  })

  it('sets http.request.body.size for string body', async () => {
    const spanMock = {
      setAttribute: vi.fn(),
      setStatus: vi.fn(),
      recordException: vi.fn(),
      end: vi.fn(),
    }
    const tracerMock = {
      startActiveSpan: vi
        .fn()
        .mockImplementation(
          (_name: string, _opts: unknown, _ctx: unknown, fn: (span: typeof spanMock) => unknown) =>
            fn(spanMock),
        ),
    }

    const { patchGlobalFetch, unpatchGlobalFetch } = await import(
      '../src/telemetry-system/fetch-instrumentation'
    )
    unpatch = unpatchGlobalFetch

    const fakeFetch = vi.fn().mockResolvedValue(new Response('ok', { status: 200 }))
    globalThis.fetch = fakeFetch

    patchGlobalFetch(tracerMock as unknown as Tracer)

    await globalThis.fetch('https://example.com/api', {
      method: 'POST',
      body: 'hello',
    })

    // 'hello' is 5 bytes in UTF-8
    expect(spanMock.setAttribute).toHaveBeenCalledWith('http.request.body.size', 5)
  })

  it('sets http.request.body.size for Uint8Array body', async () => {
    const spanMock = {
      setAttribute: vi.fn(),
      setStatus: vi.fn(),
      recordException: vi.fn(),
      end: vi.fn(),
    }
    const tracerMock = {
      startActiveSpan: vi
        .fn()
        .mockImplementation(
          (_name: string, _opts: unknown, _ctx: unknown, fn: (span: typeof spanMock) => unknown) =>
            fn(spanMock),
        ),
    }

    const { patchGlobalFetch, unpatchGlobalFetch } = await import(
      '../src/telemetry-system/fetch-instrumentation'
    )
    unpatch = unpatchGlobalFetch

    const fakeFetch = vi.fn().mockResolvedValue(new Response('ok', { status: 200 }))
    globalThis.fetch = fakeFetch

    patchGlobalFetch(tracerMock as unknown as Tracer)

    await globalThis.fetch('https://example.com/api', {
      method: 'POST',
      body: new Uint8Array([1, 2, 3, 4]),
    })

    expect(spanMock.setAttribute).toHaveBeenCalledWith('http.request.body.size', 4)
  })

  it('does not set http.request.body.size when body is absent', async () => {
    const spanMock = {
      setAttribute: vi.fn(),
      setStatus: vi.fn(),
      recordException: vi.fn(),
      end: vi.fn(),
    }
    const tracerMock = {
      startActiveSpan: vi
        .fn()
        .mockImplementation(
          (_name: string, _opts: unknown, _ctx: unknown, fn: (span: typeof spanMock) => unknown) =>
            fn(spanMock),
        ),
    }

    const { patchGlobalFetch, unpatchGlobalFetch } = await import(
      '../src/telemetry-system/fetch-instrumentation'
    )
    unpatch = unpatchGlobalFetch

    const fakeFetch = vi.fn().mockResolvedValue(new Response('ok', { status: 200 }))
    globalThis.fetch = fakeFetch

    patchGlobalFetch(tracerMock as unknown as Tracer)

    await globalThis.fetch('https://example.com/api')

    const calls = spanMock.setAttribute.mock.calls.map(c => c[0])
    expect(calls).not.toContain('http.request.body.size')
  })

  it('sets http.response.body.size from Content-Length response header', async () => {
    const spanMock = {
      setAttribute: vi.fn(),
      setStatus: vi.fn(),
      recordException: vi.fn(),
      end: vi.fn(),
    }
    const tracerMock = {
      startActiveSpan: vi
        .fn()
        .mockImplementation(
          (_name: string, _opts: unknown, _ctx: unknown, fn: (span: typeof spanMock) => unknown) =>
            fn(spanMock),
        ),
    }

    const { patchGlobalFetch, unpatchGlobalFetch } = await import(
      '../src/telemetry-system/fetch-instrumentation'
    )
    unpatch = unpatchGlobalFetch

    const fakeFetch = vi.fn().mockResolvedValue(
      new Response('hello world', {
        status: 200,
        headers: { 'content-length': '11' },
      }),
    )
    globalThis.fetch = fakeFetch

    patchGlobalFetch(tracerMock as unknown as Tracer)

    await globalThis.fetch('https://example.com/api')

    expect(spanMock.setAttribute).toHaveBeenCalledWith('http.response.body.size', 11)
  })

  it('does not set http.response.body.size when Content-Length header absent', async () => {
    const spanMock = {
      setAttribute: vi.fn(),
      setStatus: vi.fn(),
      recordException: vi.fn(),
      end: vi.fn(),
    }
    const tracerMock = {
      startActiveSpan: vi
        .fn()
        .mockImplementation(
          (_name: string, _opts: unknown, _ctx: unknown, fn: (span: typeof spanMock) => unknown) =>
            fn(spanMock),
        ),
    }

    const { patchGlobalFetch, unpatchGlobalFetch } = await import(
      '../src/telemetry-system/fetch-instrumentation'
    )
    unpatch = unpatchGlobalFetch

    const fakeFetch = vi.fn().mockResolvedValue(new Response('ok', { status: 200 }))
    globalThis.fetch = fakeFetch

    patchGlobalFetch(tracerMock as unknown as Tracer)

    await globalThis.fetch('https://example.com/api')

    const calls = spanMock.setAttribute.mock.calls.map(c => c[0])
    expect(calls).not.toContain('http.response.body.size')
  })

  it('captures http.request.header.content-type and http.request.header.accept', async () => {
    const spanMock = {
      setAttribute: vi.fn(),
      setStatus: vi.fn(),
      recordException: vi.fn(),
      end: vi.fn(),
    }
    const tracerMock = {
      startActiveSpan: vi
        .fn()
        .mockImplementation(
          (_name: string, _opts: unknown, _ctx: unknown, fn: (span: typeof spanMock) => unknown) =>
            fn(spanMock),
        ),
    }

    const { patchGlobalFetch, unpatchGlobalFetch } = await import(
      '../src/telemetry-system/fetch-instrumentation'
    )
    unpatch = unpatchGlobalFetch

    const fakeFetch = vi.fn().mockResolvedValue(new Response('ok', { status: 200 }))
    globalThis.fetch = fakeFetch

    patchGlobalFetch(tracerMock as unknown as Tracer)

    await globalThis.fetch('https://example.com/api', {
      method: 'POST',
      headers: {
        'content-type': 'application/json',
        accept: 'application/json',
      },
      body: '{}',
    })

    expect(spanMock.setAttribute).toHaveBeenCalledWith(
      'http.request.header.content-type',
      'application/json',
    )
    expect(spanMock.setAttribute).toHaveBeenCalledWith(
      'http.request.header.accept',
      'application/json',
    )
  })

  it('captures http.response.header.content-type', async () => {
    const spanMock = {
      setAttribute: vi.fn(),
      setStatus: vi.fn(),
      recordException: vi.fn(),
      end: vi.fn(),
    }
    const tracerMock = {
      startActiveSpan: vi
        .fn()
        .mockImplementation(
          (_name: string, _opts: unknown, _ctx: unknown, fn: (span: typeof spanMock) => unknown) =>
            fn(spanMock),
        ),
    }

    const { patchGlobalFetch, unpatchGlobalFetch } = await import(
      '../src/telemetry-system/fetch-instrumentation'
    )
    unpatch = unpatchGlobalFetch

    const fakeFetch = vi.fn().mockResolvedValue(
      new Response('{}', {
        status: 200,
        headers: { 'content-type': 'application/json' },
      }),
    )
    globalThis.fetch = fakeFetch

    patchGlobalFetch(tracerMock as unknown as Tracer)

    await globalThis.fetch('https://example.com/api')

    expect(spanMock.setAttribute).toHaveBeenCalledWith(
      'http.response.header.content-type',
      'application/json',
    )
  })

  it('does not set header attributes when headers are absent', async () => {
    const spanMock = {
      setAttribute: vi.fn(),
      setStatus: vi.fn(),
      recordException: vi.fn(),
      end: vi.fn(),
    }
    const tracerMock = {
      startActiveSpan: vi
        .fn()
        .mockImplementation(
          (_name: string, _opts: unknown, _ctx: unknown, fn: (span: typeof spanMock) => unknown) =>
            fn(spanMock),
        ),
    }

    const { patchGlobalFetch, unpatchGlobalFetch } = await import(
      '../src/telemetry-system/fetch-instrumentation'
    )
    unpatch = unpatchGlobalFetch

    // Use null body so the Response has no auto-set content-type header
    const fakeFetch = vi.fn().mockResolvedValue(new Response(null, { status: 200 }))
    globalThis.fetch = fakeFetch

    patchGlobalFetch(tracerMock as unknown as Tracer)

    await globalThis.fetch('https://example.com/api')

    const calls = spanMock.setAttribute.mock.calls.map(c => c[0])
    expect(calls).not.toContain('http.request.header.content-type')
    expect(calls).not.toContain('http.request.header.accept')
    expect(calls).not.toContain('http.response.header.content-type')
  })

  it('sets error.type and ERROR status on 4xx response', async () => {
    const spanMock = {
      setAttribute: vi.fn(),
      setStatus: vi.fn(),
      recordException: vi.fn(),
      end: vi.fn(),
    }
    const tracerMock = {
      startActiveSpan: vi
        .fn()
        .mockImplementation(
          (_name: string, _opts: unknown, _ctx: unknown, fn: (span: typeof spanMock) => unknown) =>
            fn(spanMock),
        ),
    }

    const { patchGlobalFetch, unpatchGlobalFetch } = await import(
      '../src/telemetry-system/fetch-instrumentation'
    )
    unpatch = unpatchGlobalFetch

    const fakeFetch = vi.fn().mockResolvedValue(new Response('not found', { status: 404 }))
    globalThis.fetch = fakeFetch

    patchGlobalFetch(tracerMock as unknown as Tracer)

    await globalThis.fetch('https://example.com/missing')

    expect(spanMock.setAttribute).toHaveBeenCalledWith('error.type', '404')
    expect(spanMock.setStatus).toHaveBeenCalledWith({ code: SpanStatusCode.ERROR })
  })
})
