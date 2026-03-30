import { describe, expect, it } from 'vitest'
import { execute, iii, sleep } from './utils'

describe('Functions Available', () => {
  it('should notify when functions change via onFunctionsAvailable', async () => {
    let latestFunctions: { function_id: string }[] = []
    let callCount = 0

    const unsub = iii.onFunctionsAvailable((functions) => {
      latestFunctions = functions
      callCount++
    })

    const fn = iii.registerFunction({ id: 'browser.test.fna.dynamic' }, async () => ({ ok: true }))

    await execute(async () => {
      if (callCount === 0) throw new Error('Not called yet')
      const found = latestFunctions.find((f) => f.function_id === 'browser.test.fna.dynamic')
      if (!found) throw new Error('Function not found in list')
    })

    expect(latestFunctions.some((f) => f.function_id === 'browser.test.fna.dynamic')).toBe(true)

    fn.unregister()
    unsub()
  })

  it('should stop receiving updates after unsubscribe', async () => {
    let callCount = 0

    const unsub = iii.onFunctionsAvailable(() => {
      callCount++
    })

    const fn1 = iii.registerFunction({ id: 'browser.test.fna.before-unsub' }, async () => ({}))

    await execute(async () => {
      if (callCount === 0) throw new Error('Not called yet')
    })

    const countBeforeUnsub = callCount
    unsub()

    const fn2 = iii.registerFunction({ id: 'browser.test.fna.after-unsub' }, async () => ({}))
    await sleep(500)

    expect(callCount).toBe(countBeforeUnsub)

    fn1.unregister()
    fn2.unregister()
  })
})
