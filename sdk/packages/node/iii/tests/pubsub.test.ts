import { describe, expect, it } from 'vitest'
import { iii, sleep } from './utils'

function uniqueTopic(prefix: string): string {
  return `${prefix}.${Date.now()}.${Math.random().toString(36).slice(2, 10)}`
}

describe('PubSub', () => {
  it('should subscribe and receive published messages', async () => {
    const topic = uniqueTopic('test.topic')
    const receivedMessages: Record<string, unknown>[] = []
    let resolveReceived: () => void
    const received = new Promise<void>(r => {
      resolveReceived = r
    })

    const fn = iii.registerFunction(
      { id: `test.pubsub.subscriber.${topic}` },
      async (data: Record<string, unknown>) => {
        receivedMessages.push(data)
        resolveReceived?.()
        return {}
      },
    )

    const trigger = iii.registerTrigger({
      type: 'subscribe',
      function_id: fn.id,
      config: { topic },
    })

    await sleep(300)

    await iii.trigger({ function_id: 'publish', payload: {
      topic,
      data: { message: 'Hello PubSub!' },
    } })

    await Promise.race([
      received,
      new Promise<never>((_, reject) =>
        setTimeout(() => reject(new Error('Timeout waiting for pubsub message')), 5000),
      ),
    ])

    expect(receivedMessages).toHaveLength(1)
    expect(receivedMessages[0]).toHaveProperty('message', 'Hello PubSub!')

    fn.unregister()
    trigger.unregister()
  })

  it('should isolate messages by topic', async () => {
    const topicA = uniqueTopic('topic.a')
    const topicB = uniqueTopic('topic.b')

    const receivedA: Record<string, unknown>[] = []
    const receivedB: Record<string, unknown>[] = []
    let resolveA: () => void
    const receivedAPromise = new Promise<void>(r => {
      resolveA = r
    })

    const fnA = iii.registerFunction(
      { id: `test.pubsub.topic_a.${topicA}` },
      async (data: Record<string, unknown>) => {
        receivedA.push(data)
        resolveA?.()
        return {}
      },
    )

    const fnB = iii.registerFunction(
      { id: `test.pubsub.topic_b.${topicB}` },
      async (data: Record<string, unknown>) => {
        receivedB.push(data)
        return {}
      },
    )

    const triggerA = iii.registerTrigger({
      type: 'subscribe',
      function_id: fnA.id,
      config: { topic: topicA },
    })

    const triggerB = iii.registerTrigger({
      type: 'subscribe',
      function_id: fnB.id,
      config: { topic: topicB },
    })

    await sleep(300)

    await iii.trigger({ function_id: 'publish', payload: {
      topic: topicA,
      data: { for: 'a' },
    } })

    await Promise.race([
      receivedAPromise,
      new Promise<never>((_, reject) =>
        setTimeout(() => reject(new Error('Timeout waiting for topic A message')), 5000),
      ),
    ])

    await sleep(200)

    expect(receivedA).toHaveLength(1)
    expect(receivedB).toHaveLength(0)

    fnA.unregister()
    fnB.unregister()
    triggerA.unregister()
    triggerB.unregister()
  })
})
