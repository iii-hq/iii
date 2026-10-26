import { EventEmitter } from 'node:events'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// Inline mock for the 'ws' module, following handshake-token.test.ts. The
// default address is the developer's own engine port: a real socket there would
// register these workers on a running engine.
class MockWebSocket extends EventEmitter {
  static readonly CONNECTING = 0
  static readonly OPEN = 1
  static readonly CLOSING = 2
  static readonly CLOSED = 3

  readyState: number = MockWebSocket.CONNECTING

  // Nothing asserts on these; the SDK only needs them to exist.
  send = vi.fn()
  ping = vi.fn()
  resume = vi.fn()

  close(): void {
    this.readyState = MockWebSocket.CLOSED
  }
  terminate(): void {
    this.readyState = MockWebSocket.CLOSED
  }
}

vi.mock('ws', () => ({
  WebSocket: MockWebSocket,
  default: { WebSocket: MockWebSocket },
}))

// setup.ts eagerly imports the SDK (and the real 'ws') before this file's mock
// takes hold, so drop the cached graph and re-import against the mock.
vi.resetModules()
const { DEFAULT_ENGINE_URL, registerWorker: connect } = await import('../src/iii')

/**
 * `registerWorker()` with no address: the supervisor that spawned this process
 * (`iii compose`, a container runtime, systemd) sets III_URL, the same way it
 * sets III_NAMESPACE and III_WORKER_NAME.
 */
describe('registerWorker — engine address resolution', () => {
  let previous: string | undefined
  // Shut every worker down after its test. Left running, its reconnect and
  // OTel timers log after the file ends, and vitest fails the run when a log
  // is still in flight as it closes the worker rpc.
  const workers: ReturnType<typeof connect>[] = []
  const registerWorker = (...args: Parameters<typeof connect>) => {
    const worker = connect(...args)
    workers.push(worker)
    return worker
  }

  beforeEach(() => {
    previous = process.env.III_URL
    delete process.env.III_URL
    // Keeps the shared OTel connection of `tests/utils` from being replaced
    // (and orphaned) by one per worker.
    vi.stubEnv('OTEL_ENABLED', 'false')
  })

  afterEach(async () => {
    await Promise.all(workers.splice(0).map((worker) => worker.shutdown()))
    vi.unstubAllEnvs()
    if (previous === undefined) {
      delete process.env.III_URL
    } else {
      process.env.III_URL = previous
    }
  })

  it('falls back to the IPv4 loopback default when nothing is set', () => {
    const worker = registerWorker()
    expect(worker.getAddress()).toBe(DEFAULT_ENGINE_URL)
    expect(DEFAULT_ENGINE_URL).toBe('ws://127.0.0.1:49134')
  })

  it('reads III_URL when no address is passed', () => {
    process.env.III_URL = 'ws://engine.example:9000'
    expect(registerWorker().getAddress()).toBe('ws://engine.example:9000')
  })

  it('an explicit address wins over III_URL', () => {
    process.env.III_URL = 'ws://from-env:1'
    expect(registerWorker('ws://explicit:2').getAddress()).toBe('ws://explicit:2')
  })

  it('ignores an empty III_URL', () => {
    process.env.III_URL = ''
    expect(registerWorker().getAddress()).toBe(DEFAULT_ENGINE_URL)
  })

  it('still accepts options when the address is omitted', () => {
    process.env.III_URL = 'ws://engine.example:9000'
    const worker = registerWorker(undefined, { workerName: 'my-worker' })
    expect(worker.getAddress()).toBe('ws://engine.example:9000')
  })
})
