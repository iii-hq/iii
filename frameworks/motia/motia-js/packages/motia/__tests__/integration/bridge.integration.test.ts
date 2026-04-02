import { TriggerAction } from 'iii-sdk'
import { getInstance, initIII } from '../../src/new/iii'
import { initTestEnv, sleep, waitForReady, waitForRegistration } from './setup'

describe('bridge integration', () => {
  beforeAll(async () => {
    initTestEnv()
    initIII({ enabled: false })
    const sdk = getInstance()
    await waitForReady(sdk)
  }, 15000)

  afterAll(async () => {
    const sdk = getInstance()
    await sdk.shutdown()
  })

  it('connects and registers function', async () => {
    const sdk = getInstance()
    const functionId = `test.echo.${Date.now()}`

    sdk.registerFunction({ id: functionId }, async (data: unknown) => {
      return { status_code: 200, body: { echoed: data } }
    })

    await waitForRegistration(sdk, functionId)

    const result = (await sdk.trigger({ function_id: functionId, payload: { message: 'hello' } })) as Record<string, any>
    expect(result).toBeDefined()
    const echoed = result.echoed ?? result.body?.echoed
    expect(echoed?.message).toBe('hello')
  }, 10000)

  it('fire-and-forget invocation via void trigger', async () => {
    const sdk = getInstance()
    const functionId = `test.receiver.${Date.now()}`
    let received: unknown = null

    sdk.registerFunction({ id: functionId }, async (data: unknown) => {
      received = data
      return {}
    })

    await waitForRegistration(sdk, functionId)

    await sdk.trigger({ function_id: functionId, payload: { value: 42 }, action: TriggerAction.Void() })

    const maxWait = 3000
    const pollInterval = 100
    const start = Date.now()
    while (received === null && Date.now() - start < maxWait) {
      await sleep(pollInterval)
    }

    expect(received).toEqual(expect.objectContaining({ value: 42 }))
  }, 10000)

  it('list registered functions', async () => {
    const sdk = getInstance()
    const func1 = `test.list.func1.${Date.now()}`
    const func2 = `test.list.func2.${Date.now()}`

    sdk.registerFunction({ id: func1 }, async () => ({}))
    sdk.registerFunction({ id: func2 }, async () => ({}))

    await waitForRegistration(sdk, func1)
    await waitForRegistration(sdk, func2)

    const result = (await sdk.trigger({ function_id: 'engine::functions::list', payload: {} })) as { functions?: { function_id: string }[] }
    const ids = result?.functions?.map((f) => f.function_id) ?? []
    expect(ids).toContain(func1)
    expect(ids).toContain(func2)
  }, 10000)

  it('invoke non-existent function rejects or returns error', async () => {
    const sdk = getInstance()
    try {
      const result = await sdk.trigger({ function_id: 'nonexistent.function.xyz', payload: {} })
      const hasError = result && typeof result === 'object' && ('error' in result || 'message' in result)
      expect(hasError || result === undefined).toBe(true)
    } catch (e) {
      expect(e).toBeDefined()
    }
  }, 5000)
})
