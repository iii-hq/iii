import { describe, expect, it } from 'vitest'
import type { ChannelReader, ChannelWriter } from '../src'
import { iii, sleep } from './utils'

describe('Data Channels', () => {
  it('should create a channel and stream data from sender to processor', async () => {
    type Record = { name: string; value: number }

    const processor = iii.registerFunction(
      { id: 'test.data.processor' },
      async (input: { label: string; reader: ChannelReader }) => {
        const chunks: Buffer[] = []
        for await (const chunk of input.reader.stream) {
          chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk))
        }

        const records: Record[] = JSON.parse(Buffer.concat(chunks).toString('utf-8'))

        const sum = records.reduce((acc, r) => acc + r.value, 0)
        const max = Math.max(...records.map(r => r.value))
        const min = Math.min(...records.map(r => r.value))

        return {
          label: input.label,
          messages: [
            { type: 'stat', key: 'count', value: records.length },
            { type: 'stat', key: 'sum', value: sum },
            { type: 'stat', key: 'average', value: sum / records.length },
            { type: 'stat', key: 'min', value: min },
            { type: 'stat', key: 'max', value: max },
          ],
        }
      },
    )

    const sender = iii.registerFunction(
      { id: 'test.data.sender' },
      async (input: { records: Record[] }) => {
        const channel = await iii.createChannel()

        const writePromise = new Promise<void>((resolve, reject) => {
          const payload = Buffer.from(JSON.stringify(input.records))
          channel.writer.stream.end(payload, (err?: Error | null) => {
            if (err) reject(err)
            else resolve()
          })
        })

        const result = await iii.trigger({ function_id: 'test.data.processor', payload: {
          label: 'metrics-batch',
          reader: channel.readerRef,
        } })

        await writePromise
        return result
      },
    )

    await sleep(300)

    try {
      const records: Record[] = [
        { name: 'cpu_usage', value: 72 },
        { name: 'memory_mb', value: 2048 },
        { name: 'disk_iops', value: 340 },
        { name: 'network_mbps', value: 95 },
        { name: 'latency_ms', value: 12 },
      ]

      // biome-ignore lint/suspicious/noExplicitAny: test code
      const result = await iii.trigger<{ records: Record[] }, any>({ function_id: 'test.data.sender', payload: { records } })

      expect(result.label).toBe('metrics-batch')
      expect(result.messages).toHaveLength(5)
      expect(result.messages).toContainEqual({ type: 'stat', key: 'count', value: 5 })
      expect(result.messages).toContainEqual({ type: 'stat', key: 'sum', value: 2567 })
      expect(result.messages).toContainEqual({ type: 'stat', key: 'average', value: 513.4 })
      expect(result.messages).toContainEqual({ type: 'stat', key: 'min', value: 12 })
      expect(result.messages).toContainEqual({ type: 'stat', key: 'max', value: 2048 })
    } finally {
      sender.unregister()
      processor.unregister()
    }
  })

  it('should create a channel and stream data from worker to coordinator', async () => {
    const worker = iii.registerFunction(
      { id: 'test.stream.worker' },
      async (input: { reader: ChannelReader; writer: ChannelWriter }) => {
        const { reader, writer } = input
        const chunks: Buffer[] = []
        let chunkCount = 0

        for await (const chunk of reader.stream) {
          chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk))
          chunkCount++
          writer.sendMessage(
            JSON.stringify({
              type: 'progress',
              chunks_received: chunkCount,
            }),
          )
        }

        const fullData = Buffer.concat(chunks).toString('utf-8')
        const words = fullData.split(/\s+/).filter(Boolean)

        writer.sendMessage(
          JSON.stringify({
            type: 'complete',
            word_count: words.length,
            byte_count: Buffer.concat(chunks).length,
          }),
        )

        writer.stream.end(
          Buffer.from(
            JSON.stringify({
              words: words.slice(0, 5),
              total: words.length,
            }),
          ),
        )

        return { status: 'done' }
      },
    )

    const coordinator = iii.registerFunction(
      { id: 'test.stream.coordinator' },
      async (input: { text: string; chunkSize: number }) => {
        const inputChannel = await iii.createChannel()
        const outputChannel = await iii.createChannel()

        const messages: unknown[] = []
        outputChannel.reader.onMessage(msg => {
          messages.push(JSON.parse(msg))
        })

        const textBuf = Buffer.from(input.text)
        const writePromise = new Promise<void>((resolve, reject) => {
          let offset = 0
          const writeNext = () => {
            while (offset < textBuf.length) {
              const end = Math.min(offset + input.chunkSize, textBuf.length)
              const chunk = textBuf.subarray(offset, end)
              offset = end

              if (!inputChannel.writer.stream.write(chunk)) {
                inputChannel.writer.stream.once('drain', writeNext)
                return
              }
            }
            inputChannel.writer.stream.end((err?: Error | null) => {
              if (err) reject(err)
              else resolve()
            })
          }
          writeNext()
        })

        const callPromise = iii.trigger({ function_id: 'test.stream.worker', payload: {
          reader: inputChannel.readerRef,
          writer: outputChannel.writerRef,
        } })

        const resultChunks: Buffer[] = []
        for await (const chunk of outputChannel.reader.stream) {
          resultChunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk))
        }

        await writePromise
        const workerResult = await callPromise
        const binaryResult = JSON.parse(Buffer.concat(resultChunks).toString('utf-8'))

        return {
          messages,
          binaryResult,
          workerResult,
        }
      },
    )

    await sleep(300)

    try {
      const text = 'The quick brown fox jumps over the lazy dog and then runs around the park'

      // biome-ignore lint/suspicious/noExplicitAny: test code
      const result = await iii.trigger<{ text: string; chunkSize: number }, any>({
        function_id: 'test.stream.coordinator',
        payload: {
          text,
          chunkSize: 10,
        },
      })

      const progressMessages = result.messages.filter(
        (m: { type: string }) => m.type === 'progress',
      )
      const completeMessage = result.messages.find((m: { type: string }) => m.type === 'complete')

      expect(progressMessages.length).toBeGreaterThan(0)
      expect(completeMessage).toBeDefined()
      expect(completeMessage.word_count).toBe(text.split(/\s+/).length)

      expect(result.binaryResult.total).toBe(text.split(/\s+/).length)
      expect(result.binaryResult.words).toHaveLength(5)
      expect(result.binaryResult.words).toEqual(['The', 'quick', 'brown', 'fox', 'jumps'])

      expect(result.workerResult.status).toBe('done')
    } finally {
      coordinator.unregister()
      worker.unregister()
    }
  })
})
