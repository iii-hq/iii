import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { WebSocketServer, type WebSocket } from 'ws'
import { registerWorker } from '../src/iii'
import type { IIIClient } from '../src/types'

describe('trigger registration error surfacing', () => {
  let wss: WebSocketServer
  let url: string
  let sdk: IIIClient | undefined
  let serverSocket: WebSocket | undefined

  beforeEach(async () => {
    wss = new WebSocketServer({ port: 0 })
    await new Promise<void>((resolve) => wss.once('listening', () => resolve()))
    const address = wss.address() as { port: number }
    url = `ws://127.0.0.1:${address.port}`
    serverSocket = undefined
    wss.on('connection', (ws) => {
      serverSocket = ws
      ws.send(JSON.stringify({ type: 'workerregistered', worker_id: 'test-worker' }))
    })
  })

  afterEach(async () => {
    if (sdk) {
      await sdk.shutdown()
    }
    vi.restoreAllMocks()
    await new Promise<void>((resolve) => wss.close(() => resolve()))
  })

  // Deterministic wait: a blind post-connect sleep flakes under CI load
  // (serverSocket still undefined at send time).
  const waitFor = async <T,>(get: () => T | undefined, ms = 5000): Promise<T> => {
    const deadline = Date.now() + ms
    for (;;) {
      const v = get()
      if (v !== undefined) return v
      if (Date.now() > deadline) throw new Error('timed out waiting for condition')
      await new Promise((r) => setTimeout(r, 10))
    }
  }

  it('logs to console.error on TriggerRegistrationResult with error', async () => {
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {})
    sdk = registerWorker(url)
    const sock = await waitFor(() => serverSocket)

    sock.send(
      JSON.stringify({
        type: 'triggerregistrationresult',
        id: 'trig-1',
        trigger_type: 'http',
        function_id: 'fn-1',
        error: {
          code: 'trigger_type_not_found',
          message:
            'Trigger type "http" not found — worker iii-http is missing. Run: iii worker add iii-http',
        },
      }),
    )

    await waitFor(() => (spy.mock.calls.length > 0 ? true : undefined))
    expect(spy).toHaveBeenCalled()
    const formatted = spy.mock.calls.map((args) => args.join(' ')).join('\n')
    expect(formatted).toContain('trig-1')
    expect(formatted).toContain('http')
    expect(formatted).toContain('iii worker add iii-http')
    spy.mockRestore()
  })

  it('does not log on TriggerRegistrationResult success (no error field)', async () => {
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {})
    sdk = registerWorker(url)
    const sock = await waitFor(() => serverSocket)

    sock.send(
      JSON.stringify({
        type: 'triggerregistrationresult',
        id: 'trig-2',
        trigger_type: 'http',
        function_id: 'fn-2',
      }),
    )

    // Negative assertion is inherently time-bounded: give handling a beat.
    await new Promise((r) => setTimeout(r, 100))
    const registrationLogs = spy.mock.calls
      .map((args) => args.join(' '))
      .filter((msg) => msg.includes('Trigger registration'))
    expect(registrationLogs).toEqual([])
    spy.mockRestore()
  })
})
