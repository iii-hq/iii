import { context, trace } from '@opentelemetry/api'
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { iii } from './utils'

const emit = vi.fn()

vi.mock('../src/telemetry-system', async () => {
  const actual = await vi.importActual<typeof import('../src/telemetry-system')>(
    '../src/telemetry-system',
  )

  return {
    ...actual,
    getLogger: () => ({ emit }),
  }
})

describe('Logger', () => {
  beforeAll(() => {
    vi.spyOn(iii, 'shutdown').mockResolvedValue(undefined)
  })

  beforeEach(() => emit.mockReset())

  it('uses the active span when no explicit trace ids are provided', async () => {
    vi.resetModules()
    const { Logger } = await import('../src/logger')

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
  })
})
