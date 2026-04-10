import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { registerWorker } from '../src/iii'
import type { FunctionInfo } from '../src/iii-types'
import type { ISdk } from '../src/types'
import { MockEngine } from './mock-websocket'

describe('Trigger Registration', () => {
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

  it('should send registertrigger with correct wire format', () => {
    sdk.registerTrigger({
      type: 'http',
      function_id: 'api::get::users',
      config: { api_path: '/users', http_method: 'GET' },
    })

    const msg = engine.findSent('registertrigger')
    expect(msg).toBeDefined()
    expect(msg?.trigger_type).toBe('http')
    expect(msg?.function_id).toBe('api::get::users')
    expect(msg?.config).toEqual({ api_path: '/users', http_method: 'GET' })
    expect(msg?.id).toBeDefined()
    expect(typeof msg?.id).toBe('string')
    // Wire format uses trigger_type, not type (type is the message type)
    expect(msg?.type).toBe('registertrigger')
  })

  it('should unregister trigger', () => {
    const trigger = sdk.registerTrigger({
      type: 'cron',
      function_id: 'scheduled::cleanup',
      config: { expression: '0 * * * *' },
    })

    const registerMsg = engine.findSent('registertrigger')
    const triggerId = registerMsg?.id as string

    trigger.unregister()

    const unregisterMsg = engine.findSent('unregistertrigger')
    expect(unregisterMsg).toBeDefined()
    expect(unregisterMsg?.id).toBe(triggerId)
  })

  it('should register multiple triggers independently', () => {
    const triggerA = sdk.registerTrigger({
      type: 'http',
      function_id: 'fn-a',
      config: { api_path: '/a', http_method: 'GET' },
    })

    const triggerB = sdk.registerTrigger({
      type: 'cron',
      function_id: 'fn-b',
      config: { expression: '*/5 * * * *' },
    })

    const registerMsgs = engine.findAllSent('registertrigger')
    expect(registerMsgs).toHaveLength(2)

    const idA = registerMsgs[0].id
    const idB = registerMsgs[1].id
    expect(idA).not.toBe(idB)

    triggerA.unregister()

    const unregisterMsgs = engine.findAllSent('unregistertrigger')
    expect(unregisterMsgs).toHaveLength(1)
    expect(unregisterMsgs[0].id).toBe(idA)

    // triggerB is still alive
    triggerB.unregister()
    const allUnregister = engine.findAllSent('unregistertrigger')
    expect(allUnregister).toHaveLength(2)
    expect(allUnregister[1].id).toBe(idB)
  })

  it('should register a function-availability trigger manually', () => {
    const handlerFunctionId = 'test.functions_available.handler'
    sdk.registerFunction(handlerFunctionId, async (_: { functions: FunctionInfo[] }) => null)
    sdk.registerTrigger({
      type: 'engine::functions-available',
      function_id: handlerFunctionId,
      config: {},
    })

    const fnMsgs = engine.findAllSent('registerfunction')
    const handlerFn = fnMsgs.find((m) => m.id === handlerFunctionId)
    expect(handlerFn).toBeDefined()

    const triggerMsgs = engine.findAllSent('registertrigger')
    const availabilityTrigger = triggerMsgs.find((m) => m.trigger_type === 'engine::functions-available')
    expect(availabilityTrigger).toBeDefined()
    expect(availabilityTrigger?.function_id).toBe(handlerFunctionId)
  })

  it('should clean up a manually-registered function-availability trigger', () => {
    const handlerFunctionId = 'test.functions_available.cleanup'
    const fn = sdk.registerFunction(handlerFunctionId, async (_: { functions: FunctionInfo[] }) => null)
    const trigger = sdk.registerTrigger({
      type: 'engine::functions-available',
      function_id: handlerFunctionId,
      config: {},
    })

    trigger.unregister()
    fn.unregister()

    const unregisterTriggerMsgs = engine.findAllSent('unregistertrigger')
    expect(unregisterTriggerMsgs.length).toBeGreaterThanOrEqual(1)
    expect(
      unregisterTriggerMsgs.some((m) => m.trigger_type === 'engine::functions-available'),
    ).toBe(true)
  })
})
