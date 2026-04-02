import { beforeAll, beforeEach, expect, it, vi } from 'vitest'
import { iii } from './utils'

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

vi.mock('../src/telemetry-system', async () => {
  const actual = await vi.importActual<typeof import('../src/telemetry-system')>(
    '../src/telemetry-system',
  )

  return {
    ...actual,
    getTracer: () => null,
    getLogger: () => ({ emit }),
    initOtel: vi.fn(),
    shutdownOtel: vi.fn(),
  }
})

beforeAll(() => {
  vi.spyOn(iii, 'shutdown').mockResolvedValue(undefined)
})

beforeEach(() => emit.mockReset())

it('keeps an active span context for handlers when tracer setup is disabled', async () => {
  vi.resetModules()
  const { registerWorker, Logger } = await import('../src/index')
  const sdk = registerWorker('ws://example.test', { otel: { enabled: false } }) as any

  sdk.registerFunction({ id: 'demo.handler' }, async () => {
    new Logger().info('inside handler')
    return { ok: true }
  })

  await sdk.functions.get('demo.handler').handler({})

  expect(emit).toHaveBeenCalledWith(
    expect.objectContaining({
      attributes: expect.objectContaining({
        trace_id: expect.any(String),
        span_id: expect.any(String),
      }),
    }),
  )
})
