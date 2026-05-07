import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { registerWorker } from '../src/iii'
import type { IIIConnectionState } from '../src/iii-constants'
import type { ISdk } from '../src/types'
import { MockEngine } from './mock-websocket'

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
