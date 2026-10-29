import { afterEach, describe, expect, it, vi } from 'vitest'
import type { IIIClient } from '../src/types'

/**
 * A local call goes through the same handler wrapper as a remote one, so the
 * `execute <fn>` span is still created (sdk.md §4.3).
 */
const spans = vi.hoisted(() => [] as { name: string; traceparent?: string }[])

vi.mock('ws', async () => (await import('./observe-fake-engine')).wsModule())
vi.mock('@iii-dev/helpers/observability/internal', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  getTracer: () => ({}),
}))
vi.mock('@iii-dev/helpers/observability', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  return {
    ...actual,
    injectTraceparent: () => '00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01',
    withSpan: async (name: string, _opts: unknown, fn: (span: unknown) => Promise<unknown>) => {
      spans.push({ name })
      return fn({})
    },
  }
})
vi.resetModules()
const fake = await import('./observe-fake-engine')
const { registerWorker } = await import('../src/index')

let sdk: IIIClient | undefined
afterEach(async () => {
  await sdk?.shutdown()
  sdk = undefined
})

describe('local dispatch and tracing', () => {
  it('creates the execute span for a local call, as for a remote one', async () => {
    sdk = registerWorker('ws://span.test', { otel: { enabled: false }, enableMetricsReporting: false })
    const socket = fake.lastSocket()
    sdk.registerFunction('math::double', async (n: number) => n * 2)
    socket.open()

    socket.deliver({ type: 'invokefunction', invocation_id: 'r-1', function_id: 'math::double', data: 1 })
    await fake.settle()
    expect(spans.map((s) => s.name)).toEqual(['execute math::double'])

    socket.deliver(fake.functionRegistered('math::double'))
    socket.clear()
    await expect(sdk.call({ function_id: 'math::double', payload: 2 })).resolves.toBe(4)
    expect(spans.map((s) => s.name)).toEqual(['execute math::double', 'execute math::double'])
    expect(socket.sent).toHaveLength(0)
  })
})
