import { describe, expect, it } from 'vitest'
import type { FunctionInfo } from '../../src/iii-types'
import { execute, iii, sleep } from './utils'

describe('Functions Available (trigger)', () => {
  it('should notify when functions change via engine::functions-available trigger', async () => {
    let latestFunctions: FunctionInfo[] = []
    let callCount = 0

    const handlerFunctionId = `browser.test.fna.handler.${crypto.randomUUID()}`
    const handlerFn = iii.registerFunction(
      handlerFunctionId,
      async ({ functions }: { functions: FunctionInfo[] }) => {
        latestFunctions = functions
        callCount++
        return null
      },
    )

    const availabilityTrigger = iii.registerTrigger({
      type: 'engine::functions-available',
      function_id: handlerFunctionId,
      config: {},
    })

    const fn = iii.registerFunction('browser.test.fna.dynamic', async () => ({ ok: true }))

    await execute(async () => {
      if (callCount === 0) throw new Error('Not called yet')
      const found = latestFunctions.find((f) => f.function_id === 'browser.test.fna.dynamic')
      if (!found) throw new Error('Function not found in list')
    })

    expect(latestFunctions.some((f) => f.function_id === 'browser.test.fna.dynamic')).toBe(true)

    fn.unregister()
    availabilityTrigger.unregister()
    handlerFn.unregister()
  })

  it('should stop receiving updates after the trigger is unregistered', async () => {
    let callCount = 0

    const handlerFunctionId = `browser.test.fna.handler.${crypto.randomUUID()}`
    const handlerFn = iii.registerFunction(
      handlerFunctionId,
      async (_: { functions: FunctionInfo[] }) => {
        callCount++
        return null
      },
    )

    const availabilityTrigger = iii.registerTrigger({
      type: 'engine::functions-available',
      function_id: handlerFunctionId,
      config: {},
    })

    const fn1 = iii.registerFunction('browser.test.fna.before-unsub', async () => ({}))

    await execute(async () => {
      if (callCount === 0) throw new Error('Not called yet')
    })

    const countBeforeUnsub = callCount
    availabilityTrigger.unregister()
    handlerFn.unregister()

    const fn2 = iii.registerFunction('browser.test.fna.after-unsub', async () => ({}))
    await sleep(500)

    expect(callCount).toBe(countBeforeUnsub)

    fn1.unregister()
    fn2.unregister()
  })
})
