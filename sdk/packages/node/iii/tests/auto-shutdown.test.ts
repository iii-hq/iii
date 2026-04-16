import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { WebSocketServer } from 'ws'
import { registerWorker } from '../src/index'
import { iii } from './utils'
import { bridgeIII } from './bridge-utils'

// Neutralise the shared SDK instances created by setup.ts. They're wired to
// real bridge URLs that aren't running in this test file; without this the
// global afterAll hook hangs for 30s trying to close their reconnecting
// WebSockets. Same trick used in exports.test.ts.
beforeAll(() => {
  vi.spyOn(iii, 'shutdown').mockResolvedValue(undefined)
  vi.spyOn(bridgeIII, 'shutdown').mockResolvedValue(undefined)
})

// The SDK's shutdown() calls `ws.removeAllListeners()` before `ws.close()`,
// so if a CONNECTING socket hasn't opened yet, the `ws` library emits an
// unhandled "WebSocket was closed before the connection was established"
// error. That's pre-existing behaviour unrelated to signal handling; silence
// it here so the test file's exit status reflects our assertions, not that
// library quirk. The ws lib emits this via EventEmitter, which surfaces as
// an uncaughtException (NOT a promise rejection) when no listener is
// attached after removeAllListeners().
const wsCloseEarlyRegex = /WebSocket was closed before the connection was established/
const swallowWsEarlyClose = (err: unknown) => {
  if (err instanceof Error && wsCloseEarlyRegex.test(err.message)) return
  throw err
}
beforeAll(() => {
  process.on('unhandledRejection', swallowWsEarlyClose)
  process.on('uncaughtException', swallowWsEarlyClose)
})
afterAll(() => {
  process.off('unhandledRejection', swallowWsEarlyClose)
  process.off('uncaughtException', swallowWsEarlyClose)
})

// Run a throwaway WebSocket server on 127.0.0.1 so the Sdk's connect()
// can complete cleanly. Using a real server instead of mocking `ws`
// dodges vitest's module-hoist ordering (setupFiles pre-load the SDK,
// which caches the real `ws` before any per-file vi.mock can swap it).
// The server accepts connections and ignores their payloads — we don't
// care about transport behaviour here, only about signal-handler wiring.
let server: WebSocketServer
let serverUrl: string

beforeAll(async () => {
  server = new WebSocketServer({ host: '127.0.0.1', port: 0 })
  // Port 0 gives us an OS-assigned port, but the listener isn't ready to
  // report its address synchronously — wait for the 'listening' event.
  await new Promise<void>((resolve, reject) => {
    server.on('listening', resolve)
    server.on('error', reject)
  })
  const addr = server.address()
  if (typeof addr === 'string' || addr === null) {
    throw new Error('WebSocketServer failed to bind to an AF_INET port')
  }
  serverUrl = `ws://127.0.0.1:${addr.port}`
  // Accept connections without doing anything — enough for the Sdk's
  // connect() to succeed and for shutdown() to close cleanly.
  server.on('connection', () => {})
})

afterAll(() => {
  // Tests fire-and-forget shutdown() on multiple Sdk instances; those WS
  // connections may still be half-open on the server side. Terminate them
  // before close() so server.close doesn't block waiting for clean closes.
  for (const client of server.clients) client.terminate()
  return new Promise<void>((resolve) => server.close(() => resolve()))
})

describe('auto-shutdown', () => {
  // Snapshot listener counts before each test so we can assert our SDK
  // instance added exactly the listeners we expect, without false positives
  // from Vitest's own or Node's built-in handlers.
  let baseline: { SIGTERM: number; SIGINT: number }

  beforeEach(() => {
    baseline = {
      SIGTERM: process.listenerCount('SIGTERM'),
      SIGINT: process.listenerCount('SIGINT'),
    }
  })

  afterEach(async () => {
    vi.restoreAllMocks()
  })

  it('installs SIGTERM and SIGINT listeners by default', async () => {
    const sdk = registerWorker(serverUrl, { otel: { enabled: false } })
    try {
      expect(process.listenerCount('SIGTERM')).toBe(baseline.SIGTERM + 1)
      expect(process.listenerCount('SIGINT')).toBe(baseline.SIGINT + 1)
    } finally {
      // Listeners are released synchronously at the top of shutdown(), so
      // we don't need to await the rest of the async teardown (OTel exporter
      // flush, WS close) which can drag for seconds under test conditions.
      sdk.shutdown().catch(() => {})
    }
  })

  it('does not install listeners when autoShutdown is false', async () => {
    const sdk = registerWorker(serverUrl, { autoShutdown: false, otel: { enabled: false } })
    try {
      expect(process.listenerCount('SIGTERM')).toBe(baseline.SIGTERM)
      expect(process.listenerCount('SIGINT')).toBe(baseline.SIGINT)
    } finally {
      // Listeners are released synchronously at the top of shutdown(), so
      // we don't need to await the rest of the async teardown (OTel exporter
      // flush, WS close) which can drag for seconds under test conditions.
      sdk.shutdown().catch(() => {})
    }
  })

  it('removes its listeners on explicit shutdown()', async () => {
    const sdk = registerWorker(serverUrl, { otel: { enabled: false } })
    expect(process.listenerCount('SIGTERM')).toBe(baseline.SIGTERM + 1)
    expect(process.listenerCount('SIGINT')).toBe(baseline.SIGINT + 1)

    // Fire shutdown but don't await — listener removal is the very first
    // thing shutdown() does, synchronously. If we awaited the full chain
    // we'd block on OTel/WS teardown, which can hang under test conditions.
    sdk.shutdown().catch(() => {})

    // If shutdown() doesn't release them, an explicit graceful cleanup
    // would still trip process.exit() on the next signal — which would be
    // both surprising and racy for users who wire their own handlers.
    expect(process.listenerCount('SIGTERM')).toBe(baseline.SIGTERM)
    expect(process.listenerCount('SIGINT')).toBe(baseline.SIGINT)
  })

  it('SIGTERM handler calls shutdown then process.exit(143)', async () => {
    // Intercept process.exit so the test doesn't actually kill the runner.
    // Throw inside the mock to halt the handler's synchronous tail and
    // surface timing to the test.
    // Record the exit call without throwing or actually exiting. The
    // handler's promise chain (`shutdown().finally(exit)`) will resolve
    // normally and we assert on the recorded call below.
    const exitSpy = vi.spyOn(process, 'exit').mockImplementation(((_code?: number) => undefined) as never)

    const sdk = registerWorker(serverUrl, { otel: { enabled: false } })
    const shutdownSpy = vi.spyOn(sdk, 'shutdown')

    // Pick the SIGTERM handler we just installed. It's the last attached
    // listener (baseline + 1); indexing by position is stable within this
    // test because beforeEach captured the snapshot right before we
    // constructed the Sdk.
    const listeners = process.listeners('SIGTERM') as Array<(signal: NodeJS.Signals) => void>
    const ourHandler = listeners[listeners.length - 1]

    // Fire it. The handler awaits shutdown() then calls process.exit(143).
    // We need to wait long enough for the async chain to reach exit; a
    // microtask flush via `await Promise.resolve()` after invoking isn't
    // quite enough because shutdown() has its own awaits, so we wait on
    // the exit spy having been called via a short polling loop.
    ourHandler('SIGTERM')

    // Drain the microtask queue a few times. shutdown() only awaits
    // synchronous work here (no network), so 20ms is plenty.
    const deadline = Date.now() + 1000
    while (Date.now() < deadline && !exitSpy.mock.calls.length) {
      await new Promise((r) => setTimeout(r, 10))
    }

    expect(shutdownSpy).toHaveBeenCalled()
    expect(exitSpy).toHaveBeenCalledWith(128 + 15)
  })

  it('SIGINT handler uses exit code 130 (128 + 2)', async () => {
    // Record the exit call without throwing or actually exiting. The
    // handler's promise chain (`shutdown().finally(exit)`) will resolve
    // normally and we assert on the recorded call below.
    const exitSpy = vi.spyOn(process, 'exit').mockImplementation(((_code?: number) => undefined) as never)

    const sdk = registerWorker(serverUrl, { otel: { enabled: false } })
    vi.spyOn(sdk, 'shutdown')

    const listeners = process.listeners('SIGINT') as Array<(signal: NodeJS.Signals) => void>
    const ourHandler = listeners[listeners.length - 1]
    ourHandler('SIGINT')

    const deadline = Date.now() + 1000
    while (Date.now() < deadline && !exitSpy.mock.calls.length) {
      await new Promise((r) => setTimeout(r, 10))
    }

    expect(exitSpy).toHaveBeenCalledWith(128 + 2)
  })

  it('shutdown failures do not block the exit path', async () => {
    // Record the exit call without throwing or actually exiting. The
    // handler's promise chain (`shutdown().finally(exit)`) will resolve
    // normally and we assert on the recorded call below.
    const exitSpy = vi.spyOn(process, 'exit').mockImplementation(((_code?: number) => undefined) as never)

    const sdk = registerWorker(serverUrl, { otel: { enabled: false } })
    // Force shutdown to reject. The handler's .catch() must swallow this
    // and still call process.exit — otherwise a misbehaving shutdown would
    // leave the worker process hanging on signal, defeating the whole point
    // of auto-shutdown in dev workflows.
    vi.spyOn(sdk, 'shutdown').mockRejectedValue(new Error('boom'))

    const listeners = process.listeners('SIGTERM') as Array<(signal: NodeJS.Signals) => void>
    listeners[listeners.length - 1]('SIGTERM')

    const deadline = Date.now() + 1000
    while (Date.now() < deadline && !exitSpy.mock.calls.length) {
      await new Promise((r) => setTimeout(r, 10))
    }

    expect(exitSpy).toHaveBeenCalledWith(128 + 15)
  })
})
