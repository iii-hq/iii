import { describe, expect, it } from 'vitest'
import { TriggerAction } from '../../src/iii'
import { execute, iii, sleep } from './utils'

describe('Bridge Operations', () => {
  it('should connect successfully', async () => {
    expect(iii).toBeDefined()
    const functions = await execute(async () => iii.listFunctions())
    expect(Array.isArray(functions)).toBe(true)
  })

  it('should register and invoke a function', async () => {
    let receivedData: Record<string, unknown> | undefined

    const fn = iii.registerFunction({ id: 'browser.test.echo' }, async (data: Record<string, unknown>) => {
      receivedData = data
      return { echoed: data }
    })

    await sleep(300)

    const result = await iii.trigger<Record<string, unknown>, { echoed: Record<string, unknown> }>({
      function_id: 'browser.test.echo',
      payload: { message: 'hello from browser' },
    })

    expect(result).toHaveProperty('echoed')
    expect(result.echoed).toHaveProperty('message', 'hello from browser')
    expect(receivedData).toHaveProperty('message', 'hello from browser')

    fn.unregister()
  })

  it('should invoke function fire-and-forget', async () => {
    let receivedData: Record<string, unknown> | undefined
    let resolveReceived: () => void
    const received = new Promise<void>((r) => {
      resolveReceived = r
    })

    const fn = iii.registerFunction({ id: 'browser.test.void-receiver' }, async (data: Record<string, unknown>) => {
      receivedData = data
      resolveReceived?.()
      return {}
    })

    await sleep(300)

    iii.trigger({
      function_id: 'browser.test.void-receiver',
      payload: { value: 42 },
      action: TriggerAction.Void(),
    })

    await Promise.race([
      received,
      new Promise<never>((_, reject) =>
        setTimeout(() => reject(new Error('Timeout waiting for fire-and-forget')), 5000),
      ),
    ])

    expect(receivedData).toHaveProperty('value', 42)

    fn.unregister()
  })

  it('should list registered functions', async () => {
    const fn1 = iii.registerFunction({ id: 'browser.test.list.func1' }, async () => ({}))
    const fn2 = iii.registerFunction({ id: 'browser.test.list.func2' }, async () => ({}))

    await sleep(300)

    const functions = await iii.listFunctions()
    const functionIds = functions.map((f) => f.function_id)

    expect(functionIds).toContain('browser.test.list.func1')
    expect(functionIds).toContain('browser.test.list.func2')

    fn1.unregister()
    fn2.unregister()
  })

  it('should reject when invoking non-existent function', async () => {
    await expect(
      iii.trigger({ function_id: 'browser.nonexistent.function', payload: {}, timeoutMs: 2000 }),
    ).rejects.toThrow()
  })

  it('should handle function returning complex data', async () => {
    const fn = iii.registerFunction({ id: 'browser.test.complex' }, async (data: { items: number[] }) => {
      return {
        count: data.items.length,
        sum: data.items.reduce((a, b) => a + b, 0),
        sorted: [...data.items].sort((a, b) => a - b),
      }
    })

    await sleep(300)

    const result = await iii.trigger<{ items: number[] }, { count: number; sum: number; sorted: number[] }>({
      function_id: 'browser.test.complex',
      payload: { items: [5, 3, 1, 4, 2] },
    })

    expect(result.count).toBe(5)
    expect(result.sum).toBe(15)
    expect(result.sorted).toEqual([1, 2, 3, 4, 5])

    fn.unregister()
  })

  it('should unregister function and reject subsequent calls', async () => {
    const fn = iii.registerFunction({ id: 'browser.test.unregister' }, async () => ({ ok: true }))

    await sleep(300)

    const result = await iii.trigger<Record<string, never>, { ok: boolean }>({
      function_id: 'browser.test.unregister',
      payload: {},
    })
    expect(result.ok).toBe(true)

    fn.unregister()
    await sleep(300)

    await expect(
      iii.trigger({ function_id: 'browser.test.unregister', payload: {}, timeoutMs: 2000 }),
    ).rejects.toThrow()
  })
})
