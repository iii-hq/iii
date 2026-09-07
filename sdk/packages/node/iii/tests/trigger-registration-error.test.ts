import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { WebSocketServer, type WebSocket } from 'ws'
import { registerWorker } from '../src/iii'
import type { IIIClient } from '../src/types'

describe('trigger registration error surfacing', () => {
  let wss: WebSocketServer
  let url: string
  let sdk: IIIClient | undefined
  let serverSocket: WebSocket | undefined
  let received: Record<string, unknown>[]

  beforeEach(async () => {
    wss = new WebSocketServer({ port: 0 })
    await new Promise<void>((resolve) => wss.once('listening', () => resolve()))
    const address = wss.address() as { port: number }
    url = `ws://127.0.0.1:${address.port}`
    serverSocket = undefined
    received = []
    wss.on('connection', (ws) => {
      serverSocket = ws
      ws.on('message', (raw) => {
        try {
          received.push(JSON.parse(raw.toString()))
        } catch {
          // Non-JSON frames are not this suite's concern.
        }
      })
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
            'Trigger type "http" not found — worker http is missing. Run: iii trigger -n <compose-daemon-namespace> compose::add worker=http',
        },
      }),
    )

    await waitFor(() => (spy.mock.calls.length > 0 ? true : undefined))
    expect(spy).toHaveBeenCalled()
    const formatted = spy.mock.calls.map((args) => args.join(' ')).join('\n')
    expect(formatted).toContain('trig-1')
    expect(formatted).toContain('http')
    expect(formatted).toContain('<compose-daemon-namespace>')
    expect(formatted).toContain('compose::add worker=http')
    spy.mockRestore()
  })

  it('records the cause on the trigger handle so a retry loop can read it', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => {})
    sdk = registerWorker(url)
    const sock = await waitFor(() => serverSocket)

    const trigger = sdk.registerTrigger({
      type: 'harness::hook::pre-generate',
      function_id: 'memory::on-pre-generate',
      config: {},
    })
    expect(trigger.registrationError).toBeUndefined()

    // The engine keys its ack by the trigger id the SDK generated, so read
    // that off the wire rather than reaching into the client's internals.
    const sent = await waitFor(
      () => received.find((m) => m.type === 'registertrigger')?.id as string | undefined,
    )

    sock.send(
      JSON.stringify({
        type: 'triggerregistrationresult',
        id: sent,
        trigger_type: 'harness::hook::pre-generate',
        function_id: 'memory::on-pre-generate',
        error: { code: 'trigger_type_not_found', message: 'Trigger type not found' },
      }),
    )

    await waitFor(() => trigger.registrationError)
    expect(trigger.registrationError?.code).toBe('trigger_type_not_found')

    // unregister drops the record: the binding no longer exists to be wrong.
    trigger.unregister()
    expect(trigger.registrationError).toBeUndefined()
  })

  it('leaves registrationError undefined for a different trigger id', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => {})
    sdk = registerWorker(url)
    const sock = await waitFor(() => serverSocket)

    const trigger = sdk.registerTrigger({ type: 'http', function_id: 'fn', config: {} })

    sock.send(
      JSON.stringify({
        type: 'triggerregistrationresult',
        id: 'some-other-trigger',
        trigger_type: 'http',
        function_id: 'fn',
        error: { code: 'trigger_type_not_found', message: 'Trigger type not found' },
      }),
    )

    await new Promise((r) => setTimeout(r, 100))
    expect(trigger.registrationError).toBeUndefined()
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
