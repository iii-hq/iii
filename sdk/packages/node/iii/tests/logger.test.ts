import { beforeEach, describe, expect, it, vi } from 'vitest'

const emit = vi.fn()

vi.mock('@iii-dev/helpers/observability', async (importOriginal) => {
  const mod = await importOriginal<typeof import('@iii-dev/helpers/observability')>()

  // Patch Logger.prototype so every Logger instance routes its otelLogger calls
  // through our spy. The mock replaces the package boundary but Logger's internal
  // binding to getLogger (a relative import inside logger.ts) cannot be intercepted
  // via package-level mocking — patching the prototype is the reliable alternative.
  Object.defineProperty(mod.Logger.prototype, 'otelLogger', {
    get() {
      return { emit }
    },
    configurable: true,
  })

  return { ...mod }
})

describe('Logger', () => {
  beforeEach(() => emit.mockReset())

  it('uses the active span when no explicit trace ids are provided', async () => {
    // Importing dynamically (not at static import time) ensures the OTel
    // context manager registered by setup.ts is visible in this execution context.
    const { Logger, initOtel, shutdownOtel } = await import('@iii-dev/helpers/observability')
    const { context, trace } = await import('@opentelemetry/api')

    // Ensure OTel context manager is registered so context.with propagates spans.
    // vi.mock('@iii-dev/helpers/observability') prevents the setup.ts-triggered initOtel
    // from registering in the current test context, so we call it explicitly here.
    initOtel({ enabled: true, engineWsUrl: 'ws://localhost:49199', serviceName: 'test' })

    try {
      const span = trace.wrapSpanContext({
        traceId: '11111111111111111111111111111111',
        spanId: '2222222222222222',
        traceFlags: 1,
      })

      await context.with(trace.setSpan(context.active(), span), async () => {
        new Logger(undefined, 'orders-service').info('hello', { ok: true })
      })

      expect(emit).toHaveBeenCalledWith(
        expect.objectContaining({
          body: 'hello',
          attributes: expect.objectContaining({
            trace_id: '11111111111111111111111111111111',
            span_id: '2222222222222222',
            'service.name': 'orders-service',
          }),
        }),
      )
    } finally {
      await shutdownOtel()
    }
  })
})
