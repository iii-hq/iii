import { describe, expect, it } from 'vitest'
import { TriggerAction } from '../src/iii'
import { execute, iii, sleep } from './utils'

describe('Bridge Operations', () => {
  it('should connect successfully', async () => {
    expect(iii).toBeDefined()
    const functions = await execute(async () => iii.listFunctions())
    expect(Array.isArray(functions)).toBe(true)
  })

  it('should register and invoke a function', async () => {
    let receivedData: Record<string, unknown> | undefined

    const fn = iii.registerFunction({ id: 'test.echo' }, async (data: Record<string, unknown>) => {
      receivedData = data
      return { echoed: data }
    })

    await sleep(300)

    const result = await iii.trigger<Record<string, unknown>, { echoed: Record<string, unknown> }>({
      function_id: 'test.echo',
      payload: { message: 'hello' },
    })

    expect(result).toHaveProperty('echoed')
    expect(result.echoed).toHaveProperty('message', 'hello')
    expect(receivedData).toHaveProperty('message', 'hello')

    fn.unregister()
  })

  it('should invoke function fire-and-forget', async () => {
    let receivedData: Record<string, unknown> | undefined
    let resolveReceived: () => void
    const received = new Promise<void>(r => {
      resolveReceived = r
    })

    const fn = iii.registerFunction(
      { id: 'test.receiver' },
      async (data: Record<string, unknown>) => {
        receivedData = data
        resolveReceived?.()
        return {}
      },
    )

    await sleep(300)

    iii.trigger({ function_id: 'test.receiver', payload: { value: 42 }, action: TriggerAction.Void() })

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
    const fn1 = iii.registerFunction({ id: 'test.list.func1' }, async () => ({}))
    const fn2 = iii.registerFunction({ id: 'test.list.func2' }, async () => ({}))

    await sleep(300)

    const functions = await iii.listFunctions()
    const functionIds = functions.map(f => f.function_id)

    expect(functionIds).toContain('test.list.func1')
    expect(functionIds).toContain('test.list.func2')

    fn1.unregister()
    fn2.unregister()
  })

  it('should reject when invoking non-existent function', async () => {
    await expect(iii.trigger({ function_id: 'nonexistent.function', payload: {}, timeoutMs: 2000 })).rejects.toThrow()
  })
})
