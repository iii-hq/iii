import { getInstance, initIII } from '../../src/new/iii'
import { initTestEnv, sleep, waitForReady, waitForRegistration } from './setup'

describe('queue integration', () => {
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

  it('enqueue delivers message to subscribed handler', async () => {
    const sdk = getInstance()
    const functionId = `test.queue.basic.${Date.now()}`
    const topic = `test-topic-${Date.now()}`
    let received: unknown = null

    sdk.registerFunction({ id: functionId }, async (data: unknown) => {
      received = data
    })
    sdk.registerTrigger({
      type: 'queue',
      function_id: functionId,
      config: { topic },
    })

    await waitForRegistration(sdk, functionId)
    await sdk.trigger({ function_id: 'enqueue', payload: { topic, data: { order: 'abc' } } })
    await sleep(1500)

    expect(received).toEqual({ order: 'abc' })
  }, 15000)

  it('handler receives exact data payload from enqueue', async () => {
    const sdk = getInstance()
    const functionId = `test.queue.payload.${Date.now()}`
    const topic = `test-topic-payload-${Date.now()}`
    const payload = { id: 'x1', count: 42, nested: { a: 1 } }
    let received: unknown = null

    sdk.registerFunction({ id: functionId }, async (data: unknown) => {
      received = data
    })
    sdk.registerTrigger({
      type: 'queue',
      function_id: functionId,
      config: { topic },
    })

    await waitForRegistration(sdk, functionId)
    await sdk.trigger({ function_id: 'enqueue', payload: { topic, data: payload } })
    await sleep(1500)

    expect(received).toEqual(payload)
  }, 15000)

  it('subscription with infrastructure config receives messages', async () => {
    const sdk = getInstance()
    const functionId = `test.queue.infra.${Date.now()}`
    const topic = `test-topic-infra-${Date.now()}`
    let received: unknown = null

    sdk.registerFunction({ id: functionId }, async (data: unknown) => {
      received = data
    })
    sdk.registerTrigger({
      type: 'queue',
      function_id: functionId,
      config: {
        topic,
        queue_config: {
          maxRetries: 5,
          type: 'standard',
          concurrency: 2,
        },
      },
    })

    await waitForRegistration(sdk, functionId)
    await sdk.trigger({ function_id: 'enqueue', payload: { topic, data: { infra: true } } })
    await sleep(1500)

    expect(received).toEqual({ infra: true })
  }, 15000)

  it('multiple subscribers on same topic - messages delivered to exactly one subscriber', async () => {
    const sdk = getInstance()
    const topic = `test-topic-multi-${Date.now()}`
    const functionId1 = `test.queue.multi1.${Date.now()}`
    const functionId2 = `test.queue.multi2.${Date.now()}`
    const received1: unknown[] = []
    const received2: unknown[] = []

    sdk.registerFunction({ id: functionId1 }, async (data: unknown) => {
      received1.push(data)
    })
    sdk.registerFunction({ id: functionId2 }, async (data: unknown) => {
      received2.push(data)
    })
    sdk.registerTrigger({
      type: 'queue',
      function_id: functionId1,
      config: { topic },
    })
    sdk.registerTrigger({
      type: 'queue',
      function_id: functionId2,
      config: { topic },
    })

    await waitForRegistration(sdk, functionId1)
    await waitForRegistration(sdk, functionId2)
    await sdk.trigger({ function_id: 'enqueue', payload: { topic, data: { msg: 1 } } })
    await sdk.trigger({ function_id: 'enqueue', payload: { topic, data: { msg: 2 } } })
    await sleep(2000)

    const total = received1.length + received2.length
    expect(total).toBe(2)
    expect([...received1, ...received2]).toContainEqual({ msg: 1 })
    expect([...received1, ...received2]).toContainEqual({ msg: 2 })
  }, 15000)

  it('condition function filters messages', async () => {
    const sdk = getInstance()
    const topic = `test-topic-cond-${Date.now()}`
    const functionId = `test.queue.cond.${Date.now()}`
    const conditionPath = `${functionId}::conditions::0`
    let handlerCalls = 0

    sdk.registerFunction({ id: functionId }, async (_data: unknown) => {
      handlerCalls += 1
    })
    sdk.registerFunction({ id: conditionPath }, async (input: { accept?: boolean }) => {
      return input?.accept === true
    })
    sdk.registerTrigger({
      type: 'queue',
      function_id: functionId,
      config: {
        topic,
        condition_function_id: conditionPath,
      },
    })

    await waitForRegistration(sdk, functionId)
    await waitForRegistration(sdk, conditionPath)
    await sdk.trigger({ function_id: 'enqueue', payload: { topic, data: { accept: false } } })
    await sdk.trigger({ function_id: 'enqueue', payload: { topic, data: { accept: true } } })
    await sleep(2000)

    expect(handlerCalls).toBe(1)
  }, 15000)
})
