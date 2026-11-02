import { describe, expect, it } from 'vitest'
import { EngineSources } from '../../src/index'
import { execute, iii, sleep } from './utils'

type FunctionRow = { function_id: string }

describe('engine::on-functions-changed (an engine-hosted source)', () => {
  it('runs an observer of it when functions change', async () => {
    let latestFunctions: FunctionRow[] = []
    let callCount = 0

    const handlerFunctionId = `browser.test.fna.handler.${crypto.randomUUID()}`
    const handlerFn = iii.registerFunction(handlerFunctionId, async ({ functions }: { functions: FunctionRow[] }) => {
      latestFunctions = functions
      callCount++
      return null
    })

    const changes = iii.observe({ function_id: EngineSources.FUNCTIONS_CHANGED, deliver: 'input' }, handlerFunctionId)
    await changes.ready()

    const fn = iii.registerFunction('browser.test.fna.dynamic', async () => ({ ok: true }))

    await execute(async () => {
      if (callCount === 0) throw new Error('Not called yet')
      const found = latestFunctions.find((f) => f.function_id === 'browser.test.fna.dynamic')
      if (!found) throw new Error('Function not found in list')
    })

    expect(latestFunctions.some((f) => f.function_id === 'browser.test.fna.dynamic')).toBe(true)

    fn.unregister()
    await changes.dispose()
    handlerFn.unregister()
  })

  it('stops running it once the observer is disposed', async () => {
    let callCount = 0

    const handlerFunctionId = `browser.test.fna.handler.${crypto.randomUUID()}`
    const handlerFn = iii.registerFunction(handlerFunctionId, async (_: { functions: FunctionRow[] }) => {
      callCount++
      return null
    })

    const changes = iii.observe({ function_id: EngineSources.FUNCTIONS_CHANGED, deliver: 'input' }, handlerFunctionId)
    await changes.ready()

    const fn1 = iii.registerFunction('browser.test.fna.before-unsub', async () => ({}))

    await execute(async () => {
      if (callCount === 0) throw new Error('Not called yet')
    })

    const countBeforeUnsub = callCount
    await changes.dispose()
    handlerFn.unregister()

    const fn2 = iii.registerFunction('browser.test.fna.after-unsub', async () => ({}))
    await sleep(500)

    expect(callCount).toBe(countBeforeUnsub)

    fn1.unregister()
    fn2.unregister()
  })
})
