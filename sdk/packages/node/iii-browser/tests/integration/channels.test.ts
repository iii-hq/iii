import { describe, expect, it } from 'vitest'
import type { ChannelReader } from '../../src/channels'
import { iii, sleep } from './utils'

describe('Data Channels', () => {
  it('should create a channel and send text messages between functions', async () => {
    const receiver = iii.registerFunction(
      'browser.test.channel.receiver',
      async (input: { reader: ChannelReader }) => {
        const messages: string[] = []

        return new Promise<{ messages: string[] }>((resolve) => {
          input.reader.onMessage((msg) => {
            messages.push(msg)
            if (messages.length === 3) {
              resolve({ messages })
            }
          })
        })
      },
    )

    const sender = iii.registerFunction('browser.test.channel.sender', async (input: { items: string[] }) => {
      const channel = await iii.createChannel()

      const resultPromise = iii.trigger({
        function_id: 'browser.test.channel.receiver',
        payload: { reader: channel.readerRef },
      })

      for (const item of input.items) {
        channel.writer.sendMessage(item)
      }

      const result = await resultPromise
      channel.writer.close()
      return result
    })

    await sleep(300)

    try {
      // biome-ignore lint/suspicious/noExplicitAny: test code
      const result = await iii.trigger<{ items: string[] }, any>({
        function_id: 'browser.test.channel.sender',
        payload: { items: ['hello', 'world', 'test'] },
      })

      expect(result.messages).toEqual(['hello', 'world', 'test'])
    } finally {
      sender.unregister()
      receiver.unregister()
    }
  })

  it('should create a channel and send binary data using sendBinary', async () => {
    const processor = iii.registerFunction(
      'browser.test.channel.binary.processor',
      async (input: { reader: ChannelReader }) => {
        const data = await input.reader.readAll()
        const text = new TextDecoder().decode(data)
        return { text, size: data.length }
      },
    )

    const sender = iii.registerFunction(
      'browser.test.channel.binary.sender',
      async (input: { text: string }) => {
        const channel = await iii.createChannel()

        const resultPromise = iii.trigger({
          function_id: 'browser.test.channel.binary.processor',
          payload: { reader: channel.readerRef },
        })

        const encoded = new TextEncoder().encode(input.text)
        channel.writer.sendBinary(encoded)
        channel.writer.close()

        return await resultPromise
      },
    )

    await sleep(300)

    try {
      // biome-ignore lint/suspicious/noExplicitAny: test code
      const result = await iii.trigger<{ text: string }, any>({
        function_id: 'browser.test.channel.binary.sender',
        payload: { text: 'Hello from binary channel!' },
      })

      expect(result.text).toBe('Hello from binary channel!')
      expect(result.size).toBe(new TextEncoder().encode('Hello from binary channel!').length)
    } finally {
      sender.unregister()
      processor.unregister()
    }
  })

  it('should stream JSON data through a channel using sendBinary', async () => {
    type Record = { name: string; value: number }

    const processor = iii.registerFunction(
      'browser.test.channel.json.processor',
      async (input: { reader: ChannelReader }) => {
        const data = await input.reader.readAll()
        const records: Record[] = JSON.parse(new TextDecoder().decode(data))

        const sum = records.reduce((acc, r) => acc + r.value, 0)
        return {
          count: records.length,
          sum,
          average: sum / records.length,
        }
      },
    )

    const sender = iii.registerFunction(
      'browser.test.channel.json.sender',
      async (input: { records: Record[] }) => {
        const channel = await iii.createChannel()

        const resultPromise = iii.trigger({
          function_id: 'browser.test.channel.json.processor',
          payload: { reader: channel.readerRef },
        })

        const encoded = new TextEncoder().encode(JSON.stringify(input.records))
        channel.writer.sendBinary(encoded)
        channel.writer.close()

        return await resultPromise
      },
    )

    await sleep(300)

    try {
      const records: Record[] = [
        { name: 'cpu', value: 72 },
        { name: 'memory', value: 2048 },
        { name: 'disk', value: 340 },
      ]

      // biome-ignore lint/suspicious/noExplicitAny: test code
      const result = await iii.trigger<{ records: Record[] }, any>({
        function_id: 'browser.test.channel.json.sender',
        payload: { records },
      })

      expect(result.count).toBe(3)
      expect(result.sum).toBe(2460)
      expect(result.average).toBeCloseTo(820, 0)
    } finally {
      sender.unregister()
      processor.unregister()
    }
  })

  it('should send progress messages through a channel writer', async () => {
    const coordinator = iii.registerFunction('browser.test.channel.progress', async () => {
      const channel = await iii.createChannel()

      const messages: unknown[] = []
      channel.reader.onMessage((msg) => {
        messages.push(JSON.parse(msg))
      })

      channel.writer.sendMessage(JSON.stringify({ type: 'progress', step: 1 }))
      channel.writer.sendMessage(JSON.stringify({ type: 'progress', step: 2 }))
      channel.writer.sendMessage(JSON.stringify({ type: 'complete' }))

      await sleep(200)
      channel.writer.close()
      channel.reader.close()

      return { messages }
    })

    await sleep(300)

    try {
      // biome-ignore lint/suspicious/noExplicitAny: test code
      const result = await iii.trigger<Record<string, never>, any>({
        function_id: 'browser.test.channel.progress',
        payload: {},
      })

      expect(result.messages).toContainEqual({ type: 'progress', step: 1 })
      expect(result.messages).toContainEqual({ type: 'progress', step: 2 })
      expect(result.messages).toContainEqual({ type: 'complete' })
    } finally {
      coordinator.unregister()
    }
  })
})
