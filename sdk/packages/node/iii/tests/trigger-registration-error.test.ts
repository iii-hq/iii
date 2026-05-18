import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { WebSocketServer, type WebSocket } from 'ws'
import { registerWorker } from '../src/iii'
import type { ISdk } from '../src/types'

describe('trigger registration error surfacing', () => {
  let wss: WebSocketServer
  let url: string
  let sdk: ISdk | undefined
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
    sdk?.shutdown?.()
    await new Promise<void>((resolve) => wss.close(() => resolve()))
  })

  it('logs to console.error on TriggerRegistrationResult with error', async () => {
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {})
    sdk = registerWorker(url)
    await new Promise((r) => setTimeout(r, 50))

    serverSocket!.send(
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

    await new Promise((r) => setTimeout(r, 20))
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
    await new Promise((r) => setTimeout(r, 50))

    serverSocket!.send(
      JSON.stringify({
        type: 'triggerregistrationresult',
        id: 'trig-2',
        trigger_type: 'http',
        function_id: 'fn-2',
      }),
    )

    await new Promise((r) => setTimeout(r, 20))
    const registrationLogs = spy.mock.calls
      .map((args) => args.join(' '))
      .filter((msg) => msg.includes('Trigger registration'))
    expect(registrationLogs).toEqual([])
    spy.mockRestore()
  })
})
