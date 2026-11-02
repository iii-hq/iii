import { describe, expect, it } from 'vitest'
import type { ChannelReader } from '../src/channel'
import { createChannel } from '../src/helpers'
import { iii, sleep } from './utils'

describe('Channel readAll', () => {
  it('should read all data from a channel using readAll', async () => {
    const processor = iii.registerFunction('test.readall.processor', async (input: { reader: ChannelReader }) => {
      const data = await input.reader.readAll()
      return { content: data.toString('utf-8'), size: data.length }
    })

    const sender = iii.registerFunction('test.readall.sender', async (input: { text: string }) => {
      const channel = await createChannel(iii)

      const writePromise = new Promise<void>((resolve, reject) => {
        const payload = Buffer.from(input.text)
        channel.writer.stream.end(payload, (err?: Error | null) => {
          if (err) reject(err)
          else resolve()
        })
      })

      const result = await iii.call({
        function_id: 'test.readall.processor',
        payload: { reader: channel.readerRef },
      })

      await writePromise
      return result
    })

    await sleep(300)

    try {
      // biome-ignore lint/suspicious/noExplicitAny: test code
      const result = await iii.call<{ text: string }, any>({
        function_id: 'test.readall.sender',
        payload: { text: 'Hello from readAll test!' },
      })

      expect(result.content).toBe('Hello from readAll test!')
      expect(result.size).toBe(Buffer.from('Hello from readAll test!').length)
    } finally {
      sender.unregister()
      processor.unregister()
    }
  })

  it('should read chunked data correctly with readAll', async () => {
    const processor = iii.registerFunction(
      'test.readall.chunked.processor',
      async (input: { reader: ChannelReader }) => {
        const data = await input.reader.readAll()
        const items = JSON.parse(data.toString('utf-8'))
        return { count: items.length, total: items.reduce((s: number, n: number) => s + n, 0) }
      },
    )

    const sender = iii.registerFunction('test.readall.chunked.sender', async (input: { numbers: number[] }) => {
      const channel = await createChannel(iii)

      const writePromise = new Promise<void>((resolve, reject) => {
        const buf = Buffer.from(JSON.stringify(input.numbers))
        let offset = 0
        const chunkSize = 8

        const writeNext = () => {
          while (offset < buf.length) {
            const end = Math.min(offset + chunkSize, buf.length)
            const chunk = buf.subarray(offset, end)
            offset = end

            if (offset >= buf.length) {
              channel.writer.stream.end(chunk, (err?: Error | null) => {
                if (err) reject(err)
                else resolve()
              })
              return
            }

            if (!channel.writer.stream.write(chunk)) {
              channel.writer.stream.once('drain', writeNext)
              return
            }
          }
        }

        writeNext()
      })

      const result = await iii.call({
        function_id: 'test.readall.chunked.processor',
        payload: { reader: channel.readerRef },
      })

      await writePromise
      return result
    })

    await sleep(300)

    try {
      const numbers = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
      // biome-ignore lint/suspicious/noExplicitAny: test code
      const result = await iii.call<{ numbers: number[] }, any>({
        function_id: 'test.readall.chunked.sender',
        payload: { numbers },
      })

      expect(result.count).toBe(10)
      expect(result.total).toBe(55)
    } finally {
      sender.unregister()
      processor.unregister()
    }
  })
})
