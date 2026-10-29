import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { registerWorker } from '../src/iii'
import { EngineFunctions, type IIIConnectionState } from '../src/iii-constants'
import type { ISdk } from '../src/types'
import { MockEngine, MockWebSocket } from './mock-websocket'

describe('addConnectionStateListener', () => {
  let engine: MockEngine
  let sdk: ISdk

  beforeEach(() => {
    engine = new MockEngine()
    engine.install()
    sdk = registerWorker('ws://test:49135')
  })

  afterEach(async () => {
    await sdk.shutdown()
    engine.uninstall()
  })

  it('returns an unsubscribe function', () => {
    const unsub = sdk.addConnectionStateListener(() => {})
    expect(typeof unsub).toBe('function')
  })

  it('fires immediately with the current state on subscribe', () => {
    const states: IIIConnectionState[] = []
    sdk.addConnectionStateListener((s) => states.push(s))
    expect(states.length).toBeGreaterThanOrEqual(1)
    expect(['connecting', 'connected', 'disconnected', 'reconnecting', 'failed']).toContain(states[0])
  })

  it('multiple listeners receive same events', async () => {
    const a: IIIConnectionState[] = []
    const b: IIIConnectionState[] = []
    sdk.addConnectionStateListener((s) => a.push(s))
    sdk.addConnectionStateListener((s) => b.push(s))

    // Trigger a transition by waiting for the open event the engine schedules.
    await engine.waitForOpen()

    expect(a.length).toBe(b.length)
    expect(a[0]).toBe(b[0])
    if (a.length > 1) {
      expect(a).toEqual(b)
    }
  })

  it('unsubscribe stops further calls', async () => {
    const calls: IIIConnectionState[] = []
    const unsub = sdk.addConnectionStateListener((s) => calls.push(s))

    await engine.waitForOpen()
    const beforeUnsub = calls.length

    unsub()

    // Trigger another transition (close should drive a state change).
    engine.socket.simulateClose()

    expect(calls.length).toBe(beforeUnsub)
  })

  it('emits transitions through connecting -> connected', async () => {
    const states: IIIConnectionState[] = []
    sdk.addConnectionStateListener((s) => states.push(s))

    await engine.waitForOpen()

    // Should have observed at minimum the initial state and a transition to connected.
    expect(states).toContain('connected')
  })
})

describe('RegistrationRejected handling', () => {
  let engine: MockEngine
  let sdk: ISdk

  beforeEach(() => {
    engine = new MockEngine()
    engine.install()
    sdk = registerWorker('ws://test:49135')
  })

  afterEach(async () => {
    await sdk.shutdown()
    engine.uninstall()
  })

  it('WORKER_NAMESPACE_CONFLICT is fatal: final state is failed, no reconnect, pending invocations rejected', async () => {
    const states: IIIConnectionState[] = []
    sdk.addConnectionStateListener((s) => states.push(s))
    await engine.waitForOpen()
    expect(states).toContain('connected')

    // A pending invocation must be rejected by the fatal rejection, not left to
    // time out.
    const pending = sdk.call({ function_id: 'orders::get', payload: { id: '1' } })
    const rejection = expect(pending).rejects.toThrow(/WORKER_NAMESPACE_CONFLICT/)

    engine.socket.simulateMessage(
      JSON.stringify({
        type: 'registrationrejected',
        code: 'WORKER_NAMESPACE_CONFLICT',
        namespace: 'default',
        worker_name: 'browser:abc',
        owner_worker_id: 'owner-1',
      }),
    )
    // The engine closes the connection after a fatal rejection. onSocketClose
    // must NOT overwrite the terminal `failed` state with `disconnected`.
    engine.socket.simulateClose()

    await rejection

    // Wait past any reconnect delay; a fatal rejection must NOT reconnect.
    await new Promise((r) => setTimeout(r, 50))

    // The FINAL state must stick as `failed` (a transient `failed` earlier in the
    // sequence is not enough — onSocketClose could clobber it back to
    // `disconnected`).
    expect(states[states.length - 1]).toBe('failed')
    expect(states).not.toContain('reconnecting')
    // Exactly one socket was ever created — no reconnect attempt.
    expect(engine.sockets.length).toBe(1)
  })

  it('an unknown rejection code is fatal, like WORKER_NAMESPACE_CONFLICT', async () => {
    const states: IIIConnectionState[] = []
    sdk.addConnectionStateListener((s) => states.push(s))
    await engine.waitForOpen()
    const errors = vi.spyOn(console, 'error').mockImplementation(() => undefined)

    engine.socket.simulateMessage(
      JSON.stringify({
        type: 'registrationrejected',
        code: 'SOMETHING_NEW',
        namespace: 'default',
        owner_worker_id: '',
      }),
    )
    engine.socket.simulateClose()
    await new Promise((r) => setTimeout(r, 20))

    expect(states[states.length - 1]).toBe('failed')
    expect(sdk.getFatalError()).toMatchObject({ code: 'SOMETHING_NEW' })
    expect(errors.mock.calls.some((c) => String(c[0]).includes('unknown code "SOMETHING_NEW"'))).toBe(true)
    expect(engine.sockets.length).toBe(1)
    errors.mockRestore()
  })

  it('FUNCTION_NAMESPACE_CONFLICT is non-fatal: stays connected', async () => {
    const states: IIIConnectionState[] = []
    sdk.addConnectionStateListener((s) => states.push(s))
    await engine.waitForOpen()
    expect(states).toContain('connected')

    engine.socket.simulateMessage(
      JSON.stringify({
        type: 'registrationrejected',
        code: 'FUNCTION_NAMESPACE_CONFLICT',
        namespace: 'orders',
        function_id: 'state::get',
        owner_worker_id: 'owner-1',
      }),
    )

    await new Promise((r) => setTimeout(r, 20))
    // No terminal transition — the connection is untouched.
    expect(states).not.toContain('failed')
    expect(states[states.length - 1]).toBe('connected')
  })
})

describe('namespace', () => {
  let engine: MockEngine

  beforeEach(() => {
    engine = new MockEngine()
    engine.install()
  })

  afterEach(async () => {
    engine.uninstall()
  })

  it('announces the configured namespace in the register-worker payload', async () => {
    const sdk = registerWorker('ws://test:49135', { namespace: 'orders' })
    await engine.waitForOpen()

    const announce = engine.findAllSent('invokefunction').find((m) => m.function_id === EngineFunctions.REGISTER_WORKER)
    expect(announce).toBeDefined()
    expect((announce?.data as { namespace?: string }).namespace).toBe('orders')

    await sdk.shutdown()
  })

  it('omits namespace from the announce when none is configured', async () => {
    const sdk = registerWorker('ws://test:49135')
    await engine.waitForOpen()

    const announce = engine.findAllSent('invokefunction').find((m) => m.function_id === EngineFunctions.REGISTER_WORKER)
    expect(announce).toBeDefined()
    expect(announce?.data as Record<string, unknown>).not.toHaveProperty('namespace')

    await sdk.shutdown()
  })

  it('serializes a per-call namespace into the InvokeFunction message', async () => {
    const sdk = registerWorker('ws://test:49135')
    await engine.waitForOpen()

    // Fire-and-forget so no response is needed; the wire message is what matters.
    await sdk.call({ function_id: 'orders::get', payload: { id: '1' }, namespace: 'orders', action: { type: 'void' } })

    const invoke = engine.findAllSent('invokefunction').find((m) => m.function_id === 'orders::get')
    expect(invoke).toBeDefined()
    expect(invoke?.namespace).toBe('orders')

    await sdk.shutdown()
  })
})

describe('shutdown', () => {
  it('while the socket is still connecting leaves no close-time error unhandled', async () => {
    // `ws` (which runs this SDK in Node) reports closing a connecting socket as
    // an error event on the next tick, and throws it when nothing handles it.
    const engine = new MockEngine({ autoOpen: false })
    engine.install()
    const sdk = registerWorker('ws://test:49135')
    const socket = engine.socket
    let unhandled: Error | undefined
    socket.close = function (this: typeof socket) {
      if (this.readyState === MockWebSocket.CONNECTING) {
        setTimeout(() => {
          if (!this.onerror) {
            unhandled = new Error('WebSocket was closed before the connection was established')
            return
          }
          this.onerror.call(this as unknown as WebSocket, new Event('error'))
        }, 0)
      }
      this.readyState = MockWebSocket.CLOSED
    }
    // Neither thrown nor reported: the SDK's own error handler (which logs) is gone too.
    const errors = vi.spyOn(console, 'error').mockImplementation(() => undefined)
    await expect(sdk.shutdown()).resolves.toBeUndefined()
    await new Promise((r) => setTimeout(r, 0)) // after the close-time error was reported
    expect(unhandled).toBeUndefined()
    expect(errors).not.toHaveBeenCalled()
    errors.mockRestore()
    engine.uninstall()
  })
})
