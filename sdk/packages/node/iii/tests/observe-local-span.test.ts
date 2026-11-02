import { afterEach, describe, expect, it, vi } from 'vitest'
import type { IIIClient } from '../src/types'

/**
 * A local call goes through the same handler wrapper as a remote one, so the
 * `execute <fn>` span is still created (sdk.md §4.3), and a deliver hook runs
 * inside it (decision 20).
 */
/** Each span's start and end, as the mocked `withSpan` saw them, and what the tests log in between. */
const events = vi.hoisted(() => [] as string[])

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
      events.push(`start ${name}`)
      try {
        return await fn({})
      } finally {
        events.push(`end ${name}`)
      }
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
  events.length = 0
})

describe('local dispatch and tracing', () => {
  it('creates the execute span for a local call, as for a remote one', async () => {
    sdk = registerWorker('ws://span.test', { otel: { enabled: false }, enableMetricsReporting: false })
    const socket = fake.lastSocket()
    sdk.registerFunction('math::double', async (n: number) => n * 2)
    socket.open()

    socket.deliver({ type: 'invokefunction', invocation_id: 'r-1', function_id: 'math::double', data: 1 })
    await fake.settle()
    const span = ['start execute math::double', 'end execute math::double']
    expect(events).toEqual(span)

    socket.deliver(fake.functionRegistered('math::double'))
    socket.clear()
    await expect(sdk.call({ function_id: 'math::double', payload: 2 })).resolves.toBe(4)
    expect(events).toEqual([...span, ...span])
    expect(socket.sent).toHaveLength(0)
  })

  it("runs the deliver hook inside the function's execute span, for a routed and a local call", async () => {
    sdk = registerWorker('ws://span.test', { otel: { enabled: false }, enableMetricsReporting: false })
    const socket = fake.lastSocket()
    sdk
      .registerFunction('orders::place', async (order: unknown) => {
        events.push('handler')
        return order
      })
      .onObserverDeliver(async (call) => {
        await Promise.resolve()
        events.push('deliver')
        return call.output
      })
    socket.open()

    socket.deliver({ type: 'invokefunction', invocation_id: 'r-1', function_id: 'orders::place', data: { id: 1 } })
    await fake.settle()
    const call = ['start execute orders::place', 'handler', 'deliver', 'end execute orders::place']
    expect(events).toEqual(call)
    expect(fake.resultOf(socket, 'r-1')).toMatchObject({ result: { id: 1 } })

    socket.deliver(fake.functionRegistered('orders::place'))
    await fake.settle()
    socket.clear()
    await expect(sdk.call({ function_id: 'orders::place', payload: { id: 2 } })).resolves.toEqual({ id: 2 })
    expect(events).toEqual([...call, ...call])
    expect(socket.sent).toHaveLength(0)
  })
})
