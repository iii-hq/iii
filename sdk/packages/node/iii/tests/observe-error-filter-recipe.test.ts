import { afterEach, describe, expect, it, vi } from 'vitest'
import type { IIIClient } from '../src/types'

/**
 * The "failed calls with their input" recipe on the SDK-host path (CONTRACT D7):
 * `capture: ['input', 'error']` + `filters: { error: { $exists: true } }`.
 * A 0.26 host evaluates the observer's filter on its local calls and reports
 * only the calls it matched, so a success never leaves the host: no
 * `invocationsobserved` frame at all. The engine side (the view it sends, and
 * the target's envelope) is engine/tests/observe_error_filter_recipe_e2e.rs.
 */
vi.mock('ws', async () => (await import('./observe-fake-engine')).wsModule())
vi.resetModules()
const fake = await import('./observe-fake-engine')
const { registerWorker } = await import('../src/index')
const { functionRegistered, observerInfo, settle, reportedItems } = fake

let sdk: IIIClient | undefined
afterEach(async () => {
  vi.restoreAllMocks()
  vi.useRealTimers()
  await sdk?.shutdown()
  sdk = undefined
})

/** The view a 0.26 engine sends for the recipe (engine test pins it, filters normalized to `$and`). */
const ENGINE_VIEW_FILTERS = { $and: [{ error: { $exists: true } }] }

async function host(filters: Record<string, unknown>) {
  fake.fakeClock()
  sdk = registerWorker('ws://local.test', {
    otel: { enabled: false },
    enableMetricsReporting: false,
    workerName: 'payments',
    reconnectionConfig: { initialDelayMs: 10, jitterFactor: 0 },
  } as never)
  const socket = fake.lastSocket()
  sdk.registerFunction('payments::charge', async (input: { amount: number; fail?: boolean }) => {
    if (input.fail) throw new Error('card declined')
    return { charged: input.amount }
  })
  socket.open(fake.ENGINE_FEATURES)
  socket.deliver(
    functionRegistered('payments::charge', [
      observerInfo('recipe', 'alerts::failed', {
        capture: ['input', 'error'],
        filters,
        fires_on: ['success', 'error'],
      }),
    ]),
  )
  await settle()
  socket.clear()
  return { sdk, socket }
}

describe('the error-filter recipe on a 0.26 host (local calls)', () => {
  it.each([
    ['as the engine sends it', ENGINE_VIEW_FILTERS],
    ['as written', { error: { $exists: true } }],
  ])('reports nothing for successes, and each failure once with input and error (%s)', async (_name, filters) => {
    const { sdk, socket } = await host(filters)
    for (let i = 0; i < 20; i++)
      await sdk.call({ function_id: 'payments::charge', payload: { amount: i } })
    await fake.elapse(10)
    // a success never reaches the engine: not even an empty report
    expect(socket.sent).toHaveLength(0)

    await sdk
      .call({ function_id: 'payments::charge', payload: { amount: 7, fail: true } })
      .catch(() => undefined)
    await sdk.call({ function_id: 'payments::charge', payload: { amount: 8 } })
    await fake.elapse(10)
    const items = reportedItems(socket)
    expect(items).toHaveLength(1)
    expect(items[0]).toMatchObject({
      function_id: 'payments::charge',
      matched: ['recipe'],
      status: 'error',
      input: { amount: 7, fail: true },
      error: { code: 'invocation_failed', message: 'card declined' },
    })
    expect(items[0]).not.toHaveProperty('output')
  })
})
