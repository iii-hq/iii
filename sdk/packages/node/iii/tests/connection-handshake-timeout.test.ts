import { type AddressInfo, createServer, type Socket } from 'node:net'
import { expect, it } from 'vitest'
import { WS_HANDSHAKE_TIMEOUT_MS } from '../src/iii-constants'
import { registerWorker } from '../src/index'

// Real `ws` transport, no mock and no engine: a raw TCP listener that accepts
// connections but never answers the WebSocket upgrade. The SDK must abandon
// the attempt after WS_HANDSHAKE_TIMEOUT_MS and re-enter reconnect backoff,
// observable as a second connection reaching the listener. Complements the
// fake-timer suite in connection-heartbeat.test.ts, which pins the SDK-side
// logic but replaces the transport. Takes ~10s of real time by design.

it('abandons a stalled handshake after WS_HANDSHAKE_TIMEOUT_MS and retries', async () => {
  const connectionTimes: number[] = []
  const clientSockets = new Set<Socket>()
  let resolveSecond: (() => void) | undefined
  const secondConnection = new Promise<void>((resolve) => {
    resolveSecond = resolve
  })

  const server = createServer((socket) => {
    clientSockets.add(socket)
    socket.on('close', () => clientSockets.delete(socket))
    connectionTimes.push(Date.now())
    if (connectionTimes.length === 2) {
      resolveSecond?.()
    }
  })
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  const port = (server.address() as AddressInfo).port

  const sdk = registerWorker(`ws://127.0.0.1:${port}`, {
    otel: { enabled: false },
    reconnectionConfig: { initialDelayMs: 100, jitterFactor: 0, maxRetries: 1 },
  })

  let guard: NodeJS.Timeout | undefined
  try {
    await Promise.race([
      secondConnection,
      new Promise((_, reject) => {
        guard = setTimeout(() => reject(new Error('no retry within 15s — handshake timeout never fired')), 15_000)
      }),
    ])

    expect(connectionTimes).toHaveLength(2)
    // The retry must come after the handshake timeout, not from an instant
    // connect failure such as ECONNREFUSED.
    expect(connectionTimes[1] - connectionTimes[0]).toBeGreaterThanOrEqual(WS_HANDSHAKE_TIMEOUT_MS - 500)
  } finally {
    if (guard) {
      clearTimeout(guard)
    }
    await sdk.shutdown()
    // The stalled sockets never close on their own and would wedge
    // server.close(), which waits for every accepted connection to end.
    for (const socket of clientSockets) {
      socket.destroy()
    }
    await new Promise<void>((resolve) => server.close(() => resolve()))
  }
}, 20_000)
