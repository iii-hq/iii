// Participant for the isolated, opt-in full-engine fragmentation integration.
import assert from 'node:assert/strict'
import { registerWorker } from '../src/iii'

const MESSAGE_LIMIT_BYTES = 64 * 1024 * 1024
const ALPHABET = '0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz'
const PATTERN_PERIOD = 97
const PATTERN_CYCLE = PATTERN_PERIOD * ALPHABET.length
const LINK = 'https://iii.dev/docs/creating-workers/channels#payload-size'
const sdk = registerWorker(process.env.JSON_SIZE_TEST_URL!, {
  workerName: 'json-size-node',
  enableMetricsReporting: false,
  otel: { enabled: false },
})

function pattern(size: number, marker: string): string {
  const seed = marker.charCodeAt(0)
  const cycle = Array.from(
    { length: PATTERN_CYCLE },
    (_, index) => ALPHABET[(index % PATTERN_PERIOD + seed) % ALPHABET.length],
  ).join('')
  return cycle.repeat(Math.ceil(size / PATTERN_CYCLE)).slice(0, size)
}

function assertPattern(value: unknown, size: number, marker: string): asserts value is string {
  if (typeof value !== 'string') throw new TypeError('expected string payload')
  assert.equal(value.length, size)
  assert.equal(value, pattern(size, marker), 'fragmented payload content/order mismatch')
}

function checkLimitMessage(message: string): void {
  const match = /^Serialized JSON envelope is (\d+) bytes; limit is 67108864 bytes\. Use channels for larger payloads: https:\/\/iii\.dev\/docs\/creating-workers\/channels#payload-size$/.exec(message)
  assert.ok(match, message)
  assert.ok(Number(match[1]) > MESSAGE_LIMIT_BYTES, `reported envelope size was ${match[1]}`)
}

sdk.registerFunction('size::node-ready', async () => true)
sdk.registerFunction('size::node', async (input: string) => input)
sdk.registerFunction('size::node-roundtrip', async ({ target, size, marker }: { target: string; size: number; marker: string }) => {
  const result = await sdk.trigger<string, string>({ function_id: target, payload: pattern(size, marker), timeoutMs: 120_000 })
  assertPattern(result, size, marker)
  console.log('JSON_SIZE_NODE_CROSS_SDK_OK')
  return true
})
sdk.registerFunction('size::node-local-limit', async () => {
  try {
    await sdk.trigger({ function_id: 'size::rust', payload: pattern(MESSAGE_LIMIT_BYTES, 'Z') })
    assert.fail('Node accepted an envelope over 64 MiB')
  } catch (error) {
    const e = error as Error & { code?: string }
    assert.equal(e.code, 'payload_too_large')
    checkLimitMessage(e.message.replace(/^payload_too_large: /, ''))
  }
  assert.equal(await sdk.trigger<string, string>({ function_id: 'size::rust', payload: 'after-node-limit' }), 'after-node-limit')
  return true
})

await new Promise(() => {})
