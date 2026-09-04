import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { registerWorker } from '../src/iii'
import type { ISdk } from '../src/types'
import { MockEngine } from './mock-websocket'

describe('trigger registration error surfacing', () => {
  let engine: MockEngine
  let sdk: ISdk

  beforeEach(async () => {
    engine = new MockEngine()
    engine.install()
    sdk = registerWorker('ws://test:49135')
    await engine.waitForOpen()
  })

  afterEach(async () => {
    await sdk.shutdown()
    engine.uninstall()
    vi.restoreAllMocks()
  })

  it('logs to console.error on triggerregistrationresult with error', () => {
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {})

    engine.sendTriggerRegistrationResult('trig-1', 'harness::hook::pre-generate', 'fn-1', {
      code: 'trigger_type_not_found',
      message: 'Trigger type not found',
    })

    const formatted = spy.mock.calls.map((args) => args.join(' ')).join('\n')
    expect(formatted).toContain('trig-1')
    expect(formatted).toContain('harness::hook::pre-generate')
    expect(formatted).toContain('Trigger type not found')
  })

  it('does not log on triggerregistrationresult success (no error field)', () => {
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {})

    engine.sendTriggerRegistrationResult('trig-2', 'http', 'fn-2')

    const registrationLogs = spy.mock.calls
      .map((args) => args.join(' '))
      .filter((msg) => msg.includes('Trigger registration'))
    expect(registrationLogs).toEqual([])
  })
})
