// Outbound JSON envelope: 16 MiB (16,777,216 bytes).
const JSON_FRAME_LIMIT_BYTES = 16 * 1024 * 1024
import assert from 'node:assert/strict'
import { registerWorker } from '../src/iii'

const sdk = registerWorker(process.env.JSON_SIZE_TEST_URL!, {
  workerName: 'json-size-node',
  enableMetricsReporting: false,
  otel: { enabled: false },
})
sdk.registerFunction('size::node', async ({ size }: { size: number }) => 'x'.repeat(size))
sdk.registerFunction('size::node-length', async (input: string) => input.length)
const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))
try {
  for (let attempt = 0; ; attempt++) {
    try {
      assert.equal(await sdk.trigger({ function_id: 'size::python', payload: { size: 1 }, timeoutMs: 1000 }), 'x')
      break
    } catch (error) {
      if (attempt >= 40) throw error
      await wait(100)
    }
  }
  const identity = (sdk as unknown as { workerId?: string }).workerId
  for (const target of ['size::python', 'size::rust']) {
    await assert.rejects(sdk.trigger({ function_id: target, payload: { size: JSON_FRAME_LIMIT_BYTES } }), { code: 'payload_too_large' })
    assert.equal(await sdk.trigger({ function_id: target, payload: { size: 2 } }), 'xx')
  }
  assert.equal(await sdk.trigger({ function_id: 'size::python-length', payload: 'y'.repeat(2 * 1024 * 1024) }), 2 * 1024 * 1024)
  assert.equal((sdk as unknown as { workerId?: string }).workerId, identity)
  console.log('JSON_SIZE_NODE_CROSS_SDK_OK')
  // Rust drives this worker after the cross-SDK assertions finish.
  await wait(8000)
} finally {
  await sdk.shutdown()
}
