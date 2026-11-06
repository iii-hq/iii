/**
 * H15: a local call of a function whose observer captures only the output
 * and filters on nothing: the delivery reads none of the input.
 *
 * `pnpm --filter iii-sdk bench observe-input-copy`
 */
import { afterAll, bench, describe, vi } from 'vitest'

vi.mock('ws', async () => (await import('../tests/observe-fake-engine')).wsModule())
vi.resetModules()
const fake = await import('../tests/observe-fake-engine')
const { registerWorker } = await import('../src/index')
const { functionRegistered, observerInfo, settle } = fake

const iii = registerWorker('ws://bench.test', {
  otel: { enabled: false },
  enableMetricsReporting: false,
  workerName: 'bench',
  reconnectionConfig: { initialDelayMs: 10, jitterFactor: 0 },
})
const socket = fake.lastSocket()
socket.send = (_data: Buffer | string, callback?: (err?: Error | null) => void) => {
  callback?.()
}
socket.open()

function payload(size: number): Record<string, unknown> {
  const doc = { k: 1, order_id: 'ord-0001', items: [] as unknown[] }
  for (let i = 0; JSON.stringify(doc).length < size; i++) {
    doc.items.push({
      id: i,
      sku: `sku-${i}`,
      name: `Item number ${i}`,
      price: 10.25 + i,
      quantity: i % 7,
      tags: ['alpha', 'beta'],
    })
  }
  return doc
}

const small = iii.registerFunction('bench::out-4k', () => ({ ok: true }))
const large = iii.registerFunction('bench::out-64k', () => ({ ok: true }))
for (const id of ['bench::out-4k', 'bench::out-64k']) {
  socket.deliver(
    functionRegistered(id, [observerInfo(`o-${id}`, 'bench::target', { capture: ['output'] })]),
  )
}
for (
  let round = 0;
  round < 1000 && (small.observers().length !== 1 || large.observers().length !== 1);
  round++
) {
  await settle()
}

afterAll(async () => {
  await iii.shutdown()
})

const p4 = payload(4 * 1024)
const p64 = payload(64 * 1024)

describe('local call, observer captures only the output', () => {
  bench(
    '4KiB',
    async () => {
      await iii.call({ function_id: 'bench::out-4k', payload: p4 })
    },
    { time: 2000 },
  )
  bench(
    '64KiB',
    async () => {
      await iii.call({ function_id: 'bench::out-64k', payload: p64 })
    },
    { time: 2000 },
  )
})
