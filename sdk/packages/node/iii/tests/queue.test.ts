import { describe, expect, it } from 'vitest'
import { TriggerAction } from '../src/index'
import { execute, iii, sleep } from './utils'

function uniqueTopic(prefix: string): string {
  return `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
}

describe('Queue Integration', () => {
  it('enqueue delivers message to registered function', async () => {
    // biome-ignore lint/suspicious/noExplicitAny: test code
    const received: any[] = []

    const consumer = iii.registerFunction(
      'test.queue.consumer',
      // biome-ignore lint/suspicious/noExplicitAny: test code
      async (input: any) => {
        received.push(input)
        return { ok: true }
      },
    )

    await sleep(300)

    try {
      const result = await iii.trigger({
        function_id: 'test.queue.consumer',
        payload: { order: 'pizza' },
        action: TriggerAction.Enqueue({ queue: 'test-orders' }),
      })

      expect(result).toHaveProperty('messageReceiptId')
      expect(typeof (result as Record<string, unknown>).messageReceiptId).toBe('string')

      await execute(async () => {
        if (received.length === 0) {
          throw new Error('Consumer has not received the message yet')
        }
      })

      expect(received).toHaveLength(1)
      expect(received[0]).toMatchObject({ order: 'pizza' })
    } finally {
      consumer.unregister()
    }
  })

  it('void trigger returns undefined immediately', async () => {
    // biome-ignore lint/suspicious/noExplicitAny: test code
    const calls: any[] = []

    const consumer = iii.registerFunction(
      'test.queue.void-consumer',
      // biome-ignore lint/suspicious/noExplicitAny: test code
      async (input: any) => {
        calls.push(input)
        return { ok: true }
      },
    )

    await sleep(300)

    try {
      const result = await iii.trigger({
        function_id: 'test.queue.void-consumer',
        payload: { msg: 'fire' },
        action: TriggerAction.Void(),
      })

      expect(result).toBeUndefined()

      await execute(async () => {
        if (calls.length === 0) {
          throw new Error('Consumer has not been called yet')
        }
      })

      expect(calls).toHaveLength(1)
      expect(calls[0]).toMatchObject({ msg: 'fire' })
    } finally {
      consumer.unregister()
    }
  })

  it('enqueue multiple messages all get processed', async () => {
    // biome-ignore lint/suspicious/noExplicitAny: test code
    const received: any[] = []
    const messageCount = 5

    const consumer = iii.registerFunction(
      'test.queue.multi-consumer',
      // biome-ignore lint/suspicious/noExplicitAny: test code
      async (input: any) => {
        received.push(input)
        return { ok: true }
      },
    )

    await sleep(300)

    try {
      for (let i = 0; i < messageCount; i++) {
        await iii.trigger({
          function_id: 'test.queue.multi-consumer',
          payload: { index: i, value: `msg-${i}` },
          action: TriggerAction.Enqueue({ queue: 'test-multi' }),
        })
      }

      await execute(async () => {
        if (received.length < messageCount) {
          throw new Error(`Only ${received.length}/${messageCount} messages received`)
        }
      })

      expect(received).toHaveLength(messageCount)

      for (let i = 0; i < messageCount; i++) {
        expect(received).toContainEqual(expect.objectContaining({ index: i, value: `msg-${i}` }))
      }
    } finally {
      consumer.unregister()
    }
  })

  it('standard queue with concurrency 1 preserves message order', async () => {
    const received: number[] = []
    const messageCount = 5

    const consumer = iii.registerFunction(
      'test.queue.sequential-consumer',
      // biome-ignore lint/suspicious/noExplicitAny: test code
      async (input: any) => {
        received.push(input.index)
        return { ok: true }
      },
    )

    await sleep(300)

    try {
      for (let i = 0; i < messageCount; i++) {
        await iii.trigger({
          function_id: 'test.queue.sequential-consumer',
          payload: { index: i },
          action: TriggerAction.Enqueue({ queue: 'test-sequential' }),
        })
      }

      await execute(async () => {
        if (received.length < messageCount) {
          throw new Error(`Only ${received.length}/${messageCount} messages received`)
        }
      })

      expect(received).toEqual([0, 1, 2, 3, 4])
    } finally {
      consumer.unregister()
    }
  })

  it('fifo queue with 2 message groups preserves per-group ordering', async () => {
    // biome-ignore lint/suspicious/noExplicitAny: test code
    const received: any[] = []
    const messagesPerGroup = 5

    const consumer = iii.registerFunction(
      'test.queue.fifo-groups-consumer',
      // biome-ignore lint/suspicious/noExplicitAny: test code
      async (input: any) => {
        received.push({ group_id: input.group_id, index: input.index })
        return { ok: true }
      },
    )

    await sleep(300)

    try {
      // Interleave messages from two groups: A0, B0, A1, B1, ...
      for (let i = 0; i < messagesPerGroup; i++) {
        await iii.trigger({
          function_id: 'test.queue.fifo-groups-consumer',
          payload: { group_id: 'group-a', index: i },
          action: TriggerAction.Enqueue({ queue: 'test-fifo-groups' }),
        })
        await iii.trigger({
          function_id: 'test.queue.fifo-groups-consumer',
          payload: { group_id: 'group-b', index: i },
          action: TriggerAction.Enqueue({ queue: 'test-fifo-groups' }),
        })
      }

      const totalMessages = messagesPerGroup * 2

      await execute(async () => {
        if (received.length < totalMessages) {
          throw new Error(`Only ${received.length}/${totalMessages} messages received`)
        }
      })

      expect(received).toHaveLength(totalMessages)

      // Extract per-group ordering and verify each group's messages arrived in order
      const groupA = received.filter(m => m.group_id === 'group-a').map(m => m.index)
      const groupB = received.filter(m => m.group_id === 'group-b').map(m => m.index)

      expect(groupA).toEqual([0, 1, 2, 3, 4])
      expect(groupB).toEqual([0, 1, 2, 3, 4])
    } finally {
      consumer.unregister()
    }
  })

  it('durable subscriber receives published message', async () => {
    // Ported from motia-js integration test: queue#enqueue delivers message to subscribed handler
    const topic = uniqueTopic('test-durable-basic')
    const functionId = `test.queue.durable.basic.${Date.now()}`
    let received: Record<string, unknown> | null = null
    let resolveReceived: () => void
    const receivedPromise = new Promise<void>(r => {
      resolveReceived = r
    })

    const fn = iii.registerFunction(functionId, async (data: Record<string, unknown>) => {
      received = data
      resolveReceived?.()
      return { ok: true }
    })
    const trigger = iii.registerTrigger({
      type: 'durable:subscriber',
      function_id: fn.id,
      config: { topic },
    })

    await sleep(300)

    try {
      await iii.trigger({
        function_id: 'iii::durable::publish',
        payload: { topic, data: { order: 'abc' } },
      })

      await Promise.race([
        receivedPromise,
        new Promise<never>((_, reject) =>
          setTimeout(() => reject(new Error('Timeout waiting for durable subscriber')), 5000),
        ),
      ])

      expect(received).toEqual({ order: 'abc' })
    } finally {
      trigger.unregister()
      fn.unregister()
    }
  })

  it('durable subscriber receives exact nested payload', async () => {
    // Ported from motia-js integration test: queue#handler receives exact data payload from enqueue
    const topic = uniqueTopic('test-durable-payload')
    const functionId = `test.queue.durable.payload.${Date.now()}`
    const payload = { id: 'x1', count: 42, nested: { a: 1 } }
    let received: unknown = null
    let resolveReceived: () => void
    const receivedPromise = new Promise<void>(r => {
      resolveReceived = r
    })

    const fn = iii.registerFunction(functionId, async (data: Record<string, unknown>) => {
      received = data
      resolveReceived?.()
      return { ok: true }
    })
    const trigger = iii.registerTrigger({
      type: 'durable:subscriber',
      function_id: fn.id,
      config: { topic },
    })

    await sleep(300)

    try {
      await iii.trigger({
        function_id: 'iii::durable::publish',
        payload: { topic, data: payload },
      })

      await Promise.race([
        receivedPromise,
        new Promise<never>((_, reject) =>
          setTimeout(() => reject(new Error('Timeout waiting for durable subscriber payload')), 5000),
        ),
      ])

      expect(received).toEqual(payload)
    } finally {
      trigger.unregister()
      fn.unregister()
    }
  })

  it('durable subscriber with queue_config receives messages', async () => {
    // Ported from motia-js integration test: queue#subscription with infrastructure config receives messages
    const topic = uniqueTopic('test-durable-infra')
    const functionId = `test.queue.durable.infra.${Date.now()}`
    let received: Record<string, unknown> | null = null
    let resolveReceived: () => void
    const receivedPromise = new Promise<void>(r => {
      resolveReceived = r
    })

    const fn = iii.registerFunction(functionId, async (data: Record<string, unknown>) => {
      received = data
      resolveReceived?.()
      return { ok: true }
    })
    const trigger = iii.registerTrigger({
      type: 'durable:subscriber',
      function_id: fn.id,
      config: {
        topic,
        queue_config: {
          maxRetries: 5,
          type: 'standard',
          concurrency: 2,
        },
      },
    })

    await sleep(300)

    try {
      await iii.trigger({
        function_id: 'iii::durable::publish',
        payload: { topic, data: { infra: true } },
      })

      await Promise.race([
        receivedPromise,
        new Promise<never>((_, reject) =>
          setTimeout(() => reject(new Error('Timeout waiting for durable subscriber (infra)')), 5000),
        ),
      ])

      expect(received).toEqual({ infra: true })
    } finally {
      trigger.unregister()
      fn.unregister()
    }
  })

  it('multiple durable subscribers on same topic each receive every message (fan-out)', async () => {
    // Ported from motia-js integration test: queue#multiple subscribers on same topic - each function receives every message (fan-out)
    const topic = uniqueTopic('test-durable-fanout')
    const functionId1 = `test.queue.durable.multi1.${Date.now()}`
    const functionId2 = `test.queue.durable.multi2.${Date.now()}`
    const received1: Record<string, unknown>[] = []
    const received2: Record<string, unknown>[] = []

    const fn1 = iii.registerFunction(functionId1, async (data: Record<string, unknown>) => {
      received1.push(data)
      return { ok: true }
    })
    const fn2 = iii.registerFunction(functionId2, async (data: Record<string, unknown>) => {
      received2.push(data)
      return { ok: true }
    })
    const trigger1 = iii.registerTrigger({
      type: 'durable:subscriber',
      function_id: fn1.id,
      config: { topic },
    })
    const trigger2 = iii.registerTrigger({
      type: 'durable:subscriber',
      function_id: fn2.id,
      config: { topic },
    })

    await sleep(500)

    try {
      await iii.trigger({
        function_id: 'iii::durable::publish',
        payload: { topic, data: { msg: 1 } },
      })
      await iii.trigger({
        function_id: 'iii::durable::publish',
        payload: { topic, data: { msg: 2 } },
      })

      await execute(async () => {
        if (received1.length < 2 || received2.length < 2) {
          throw new Error(
            `fan-out incomplete: fn1=${received1.length}/2 fn2=${received2.length}/2`,
          )
        }
      })

      expect(received1).toHaveLength(2)
      expect(received2).toHaveLength(2)
      expect(received1).toContainEqual({ msg: 1 })
      expect(received1).toContainEqual({ msg: 2 })
      expect(received2).toContainEqual({ msg: 1 })
      expect(received2).toContainEqual({ msg: 2 })
    } finally {
      trigger1.unregister()
      trigger2.unregister()
      fn1.unregister()
      fn2.unregister()
    }
  })

  it('durable subscriber condition function filters messages', async () => {
    // Ported from motia-js integration test: queue#condition function filters messages
    const topic = uniqueTopic('test-durable-cond')
    const functionId = `test.queue.durable.cond.${Date.now()}`
    const conditionFunctionId = `${functionId}::conditions::0`
    let handlerCalls = 0
    let resolveFirstCall: () => void
    const firstCall = new Promise<void>(r => {
      resolveFirstCall = r
    })

    const fn = iii.registerFunction(functionId, async (_data: Record<string, unknown>) => {
      handlerCalls += 1
      resolveFirstCall?.()
      return { ok: true }
    })
    const conditionFn = iii.registerFunction(
      conditionFunctionId,
      async (input: Record<string, unknown>) => {
        return input?.accept === true
      },
    )
    const trigger = iii.registerTrigger({
      type: 'durable:subscriber',
      function_id: fn.id,
      config: {
        topic,
        condition_function_id: conditionFn.id,
      },
    })

    await sleep(500)

    try {
      await iii.trigger({
        function_id: 'iii::durable::publish',
        payload: { topic, data: { accept: false } },
      })
      await iii.trigger({
        function_id: 'iii::durable::publish',
        payload: { topic, data: { accept: true } },
      })

      await Promise.race([
        firstCall,
        new Promise<never>((_, reject) =>
          setTimeout(() => reject(new Error('Timeout waiting for accepted message')), 5000),
        ),
      ])

      // Wait a bit extra to ensure the rejected message isn't still in flight.
      await sleep(500)

      expect(handlerCalls).toBe(1)
    } finally {
      trigger.unregister()
      fn.unregister()
      conditionFn.unregister()
    }
  })

  it('chained enqueue - function A enqueues to function B', async () => {
    // biome-ignore lint/suspicious/noExplicitAny: test code
    const chainedReceived: any[] = []

    const functionB = iii.registerFunction(
      'test.queue.chain-b',
      // biome-ignore lint/suspicious/noExplicitAny: test code
      async (input: any) => {
        chainedReceived.push(input)
        return { ok: true }
      },
    )

    const functionA = iii.registerFunction(
      'test.queue.chain-a',
      // biome-ignore lint/suspicious/noExplicitAny: test code
      async (input: any) => {
        await iii.trigger({
          function_id: 'test.queue.chain-b',
          payload: { ...input, chained: true },
          action: TriggerAction.Enqueue({ queue: 'test-chain' }),
        })
        return input
      },
    )

    await sleep(300)

    try {
      await iii.trigger({
        function_id: 'test.queue.chain-a',
        payload: { origin: 'test', data: 42 },
        action: TriggerAction.Enqueue({ queue: 'test-chain-entry' }),
      })

      await execute(async () => {
        if (chainedReceived.length === 0) {
          throw new Error('Function B has not received the chained message yet')
        }
      })

      expect(chainedReceived).toHaveLength(1)
      expect(chainedReceived[0]).toMatchObject({ origin: 'test', data: 42, chained: true })
    } finally {
      functionA.unregister()
      functionB.unregister()
    }
  })
})
