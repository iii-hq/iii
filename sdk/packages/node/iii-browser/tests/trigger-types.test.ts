import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { registerWorker } from '../src/iii'
import type { ISdk } from '../src/types'
import { MockEngine } from './mock-websocket'

describe('Trigger Types', () => {
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
  })

  it('should send registertriggertype message', () => {
    sdk.registerTriggerType(
      { id: 'webhook', description: 'Incoming webhook trigger' },
      { registerTrigger: async () => {}, unregisterTrigger: async () => {} },
    )

    const msg = engine.findSent('registertriggertype')
    expect(msg).toBeDefined()
    expect(msg!.id).toBe('webhook')
    expect(msg!.description).toBe('Incoming webhook trigger')
  })

  it('should return TriggerTypeRef with convenience methods', () => {
    const ref = sdk.registerTriggerType(
      { id: 'webhook', description: 'Webhook' },
      { registerTrigger: async () => {}, unregisterTrigger: async () => {} },
    )

    expect(ref.id).toBe('webhook')
    expect(typeof ref.registerTrigger).toBe('function')
    expect(typeof ref.registerFunction).toBe('function')
    expect(typeof ref.unregister).toBe('function')
  })

  it('should register trigger with correct type via ref.registerTrigger', () => {
    const ref = sdk.registerTriggerType(
      { id: 'webhook', description: 'Webhook' },
      { registerTrigger: async () => {}, unregisterTrigger: async () => {} },
    )

    ref.registerTrigger('my-handler', { url: '/hooks/test' })

    const triggerMsg = engine.findSent('registertrigger')
    expect(triggerMsg).toBeDefined()
    expect(triggerMsg!.trigger_type).toBe('webhook')
    expect(triggerMsg!.function_id).toBe('my-handler')
    expect(triggerMsg!.config).toEqual({ url: '/hooks/test' })
  })

  it('should register function + trigger via ref.registerFunction', () => {
    const ref = sdk.registerTriggerType(
      { id: 'webhook', description: 'Webhook' },
      { registerTrigger: async () => {}, unregisterTrigger: async () => {} },
    )

    const fnRef = ref.registerFunction(
      'webhook::handler',
      async (data) => ({ received: data }),
      { url: '/hooks/handler', methods: ['POST'] },
    )

    expect(fnRef.id).toBe('webhook::handler')

    const fnMsg = engine.findAllSent('registerfunction').find((m) => m.id === 'webhook::handler')
    expect(fnMsg).toBeDefined()

    const triggerMsg = engine.findSent('registertrigger')
    expect(triggerMsg).toBeDefined()
    expect(triggerMsg!.trigger_type).toBe('webhook')
    expect(triggerMsg!.function_id).toBe('webhook::handler')
  })

  it('should unregister trigger type via ref.unregister', () => {
    const ref = sdk.registerTriggerType(
      { id: 'webhook', description: 'Webhook' },
      { registerTrigger: async () => {}, unregisterTrigger: async () => {} },
    )

    ref.unregister()

    const msg = engine.findSent('unregistertriggertype')
    expect(msg).toBeDefined()
    expect(msg!.id).toBe('webhook')
  })

  it('should call handler.registerTrigger when engine sends RegisterTrigger', async () => {
    const registerTrigger = vi.fn().mockResolvedValue(undefined)
    const unregisterTrigger = vi.fn().mockResolvedValue(undefined)

    sdk.registerTriggerType(
      { id: 'webhook', description: 'Webhook' },
      { registerTrigger, unregisterTrigger },
    )

    engine.sendRegisterTrigger('webhook', 'trigger-1', 'handler-fn', { url: '/test' })

    await new Promise<void>((r) => setTimeout(r, 10))

    expect(registerTrigger).toHaveBeenCalledWith({
      id: 'trigger-1',
      function_id: 'handler-fn',
      config: { url: '/test' },
    })

    const resultMsg = engine.findSent('triggerregistrationresult')
    expect(resultMsg).toBeDefined()
    expect(resultMsg!.id).toBe('trigger-1')
    expect(resultMsg!.trigger_type).toBe('webhook')
    expect(resultMsg!.error).toBeUndefined()
  })

  it('should send error result when handler throws', async () => {
    sdk.registerTriggerType(
      { id: 'webhook', description: 'Webhook' },
      {
        registerTrigger: async () => { throw new Error('invalid config') },
        unregisterTrigger: async () => {},
      },
    )

    engine.sendRegisterTrigger('webhook', 'trigger-2', 'handler-fn', {})

    await new Promise<void>((r) => setTimeout(r, 10))

    const resultMsg = engine.findSent('triggerregistrationresult')
    expect(resultMsg).toBeDefined()
    const error = resultMsg!.error as Record<string, unknown>
    expect(error.code).toBe('trigger_registration_failed')
    expect(error.message).toBe('invalid config')
  })

  it('should send error for unknown trigger type', async () => {
    engine.sendRegisterTrigger('nonexistent', 'trigger-3', 'handler-fn', {})

    await new Promise<void>((r) => setTimeout(r, 10))

    const resultMsg = engine.findSent('triggerregistrationresult')
    expect(resultMsg).toBeDefined()
    const error = resultMsg!.error as Record<string, unknown>
    expect(error.code).toBe('trigger_type_not_found')
  })
})
