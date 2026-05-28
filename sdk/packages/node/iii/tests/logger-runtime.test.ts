import { beforeEach, expect, it, vi } from 'vitest'

const emit = vi.fn()

vi.mock('ws', () => {
  const MockWebSocket = vi.fn().mockImplementation(() => ({
    on: vi.fn(),
    close: vi.fn(),
    send: vi.fn(),
    readyState: 0,
  }))
  return { WebSocket: MockWebSocket, default: { WebSocket: MockWebSocket } }
})

vi.mock('@iii-dev/observability', async (importOriginal) => {
  const mod = await importOriginal<typeof import('@iii-dev/observability')>()

  // Patch Logger.prototype so every Logger instance routes its otelLogger calls
  // through our spy. See logger.test.ts for the full explanation of why this is
  // needed instead of mocking getLogger at the package boundary.
  Object.defineProperty(mod.Logger.prototype, 'otelLogger', {
    get() {
      return { emit }
    },
    configurable: true,
  })

  return { ...mod }
})

// Return null tracer so iii.ts falls back to the synthetic-span path when
// OTel is disabled, which is the behaviour this test exercises. iii.ts reads
// getTracer from the internal entry point, so the override lives here.
vi.mock('@iii-dev/observability/internal', async (importOriginal) => {
  const mod = await importOriginal<typeof import('@iii-dev/observability/internal')>()
  return {
    ...mod,
    getTracer: () => null,
  }
})

beforeEach(() => emit.mockReset())

it('keeps an active span context for handlers when tracer setup is disabled', async () => {
  vi.resetModules()
  const { registerWorker, Logger } = await import('../src/index')
  const { initOtel, shutdownOtel } = await import('@iii-dev/observability')

  // Register the AsyncLocalStorage context manager so that context.with
  // (used by the synthetic-span code path in iii.ts) correctly propagates
  // the span into the handler's execution context.
  initOtel({ enabled: true, engineWsUrl: 'ws://localhost:49199', serviceName: 'test' })

  try {
    const sdk = registerWorker('ws://example.test', { otel: { enabled: false } }) as any

    sdk.registerFunction('demo::handler', async () => {
      new Logger().info('inside handler')
      return { ok: true }
    })

    await sdk.functions.get('demo::handler').handler({})

    expect(emit).toHaveBeenCalledWith(
      expect.objectContaining({
        attributes: expect.objectContaining({
          trace_id: expect.any(String),
          span_id: expect.any(String),
        }),
      }),
    )
  } finally {
    await shutdownOtel()
  }
})
