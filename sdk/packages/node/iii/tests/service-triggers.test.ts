import { describe, expect, it } from 'vitest'
import type { ChannelReader } from '../src/channel'
import { createChannel } from '../src/helpers'
import { EngineFunctions } from '../src/iii-constants'
import { iii, sleep } from './utils'

type RegisteredTriggerRow = {
  id: string
  trigger_type: string
  function_id: string
  worker_name: string
  config_summary: string
}

type TriggerTypeRow = {
  id: string
  worker_name: string
  description: string
}

describe('List Registered Triggers', () => {
  it('should list registered trigger instances', async () => {
    const fn = iii.registerFunction('test.triggers.list.func', async () => ({ ok: true }))

    const trigger = iii.registerTrigger({
      type: 'http',
      function_id: fn.id,
      config: { api_path: 'test/list-triggers', http_method: 'GET' },
    })

    await sleep(500)

    const { registered_triggers } = await iii.trigger<
      { include_internal: boolean },
      { registered_triggers: RegisteredTriggerRow[] }
    >({
      function_id: EngineFunctions.LIST_REGISTERED_TRIGGERS,
      payload: { include_internal: false },
    })
    expect(Array.isArray(registered_triggers)).toBe(true)

    const found = registered_triggers.find((t) => t.function_id === 'test.triggers.list.func')
    expect(found).toBeDefined()
    expect(found?.trigger_type).toBe('http')
    // Each row carries the resolved owning worker name and a truncated
    // config summary (use the info function for the full config block).
    expect(typeof found?.worker_name).toBe('string')
    expect(typeof found?.config_summary).toBe('string')

    trigger.unregister()
    fn.unregister()
  })

  it('should return an array even when no registered triggers exist', async () => {
    const { registered_triggers } = await iii.trigger<
      { include_internal: boolean },
      { registered_triggers: RegisteredTriggerRow[] }
    >({
      function_id: EngineFunctions.LIST_REGISTERED_TRIGGERS,
      payload: { include_internal: false },
    })
    expect(Array.isArray(registered_triggers)).toBe(true)
  })
})

describe('List Triggers (trigger TYPES)', () => {
  it('should list trigger types (templates), not instances', async () => {
    const { triggers } = await iii.trigger<
      { include_internal: boolean },
      { triggers: TriggerTypeRow[] }
    >({
      function_id: EngineFunctions.LIST_TRIGGERS,
      payload: { include_internal: false },
    })
    expect(Array.isArray(triggers)).toBe(true)

    // The engine always registers the built-in 'http' trigger type.
    const httpType = triggers.find((t) => t.id === 'http')
    expect(httpType).toBeDefined()
    expect(httpType?.description).toBeDefined()
    // Schemas are NOT on the list response — they live on
    // `engine::triggers::info` only.
    expect((httpType as unknown as { configuration_schema?: unknown }).configuration_schema).toBeUndefined()
  })

  it('should accept includeInternal parameter', async () => {
    const { triggers } = await iii.trigger<
      { include_internal: boolean },
      { triggers: TriggerTypeRow[] }
    >({
      function_id: EngineFunctions.LIST_TRIGGERS,
      payload: { include_internal: false },
    })
    const { triggers: withInternal } = await iii.trigger<
      { include_internal: boolean },
      { triggers: TriggerTypeRow[] }
    >({
      function_id: EngineFunctions.LIST_TRIGGERS,
      payload: { include_internal: true },
    })
    expect(Array.isArray(triggers)).toBe(true)
    expect(Array.isArray(withInternal)).toBe(true)
    expect(withInternal.length).toBeGreaterThanOrEqual(triggers.length)
  })
})

describe('TriggerTypeRef', () => {
  it('should return a TriggerTypeRef from registerTriggerType', async () => {
    const ref = iii.registerTriggerType(
      { id: 'test.trigger-type-ref', description: 'Test trigger type ref' },
      {
        async registerTrigger() {},
        async unregisterTrigger() {},
      },
    )

    expect(ref).toBeDefined()
    expect(ref.id).toBe('test.trigger-type-ref')
    expect(typeof ref.registerTrigger).toBe('function')
    expect(typeof ref.registerFunction).toBe('function')
    expect(typeof ref.unregister).toBe('function')

    ref.unregister()
  })

  it('should register a trigger via TriggerTypeRef', async () => {
    const ref = iii.registerTriggerType(
      { id: 'test.tt-ref-trigger', description: 'Test ref trigger' },
      {
        async registerTrigger() {},
        async unregisterTrigger() {},
      },
    )

    const fn = iii.registerFunction('test.tt-ref-trigger.fn', async () => ({ ok: true }))

    await sleep(300)

    const trigger = ref.registerTrigger('test.tt-ref-trigger.fn', { some: 'config' })
    expect(trigger).toBeDefined()
    expect(typeof trigger.unregister).toBe('function')

    trigger.unregister()
    fn.unregister()
    ref.unregister()
  })

  it('should register a function with trigger via TriggerTypeRef', async () => {
    const ref = iii.registerTriggerType(
      { id: 'test.tt-ref-fn', description: 'Test ref function' },
      {
        async registerTrigger() {},
        async unregisterTrigger() {},
      },
    )

    const fnRef = ref.registerFunction(
      'test.tt-ref-fn.handler',
      async () => ({ ok: true }),
      { path: '/test' },
    )

    expect(fnRef).toBeDefined()
    expect(fnRef.id).toBe('test.tt-ref-fn.handler')
    expect(typeof fnRef.unregister).toBe('function')

    await sleep(300)

    const result = await iii.trigger<Record<string, never>, { ok: boolean }>({
      function_id: 'test.tt-ref-fn.handler',
      payload: {},
    })
    expect(result.ok).toBe(true)

    fnRef.unregister()
    ref.unregister()
  })

  it('should unregister the trigger type via TriggerTypeRef', () => {
    const ref = iii.registerTriggerType(
      { id: 'test.tt-ref-unreg', description: 'Test ref unregister' },
      {
        async registerTrigger() {},
        async unregisterTrigger() {},
      },
    )

    // Should not throw
    expect(() => ref.unregister()).not.toThrow()
  })
})

describe('Channel readAll', () => {
  it('should read all data from a channel using readAll', async () => {
    const processor = iii.registerFunction(
      'test.readall.processor',
      async (input: { reader: ChannelReader }) => {
        const data = await input.reader.readAll()
        return { content: data.toString('utf-8'), size: data.length }
      },
    )

    const sender = iii.registerFunction('test.readall.sender', async (input: { text: string }) => {
      const channel = await createChannel(iii)

      const writePromise = new Promise<void>((resolve, reject) => {
        const payload = Buffer.from(input.text)
        channel.writer.stream.end(payload, (err?: Error | null) => {
          if (err) reject(err)
          else resolve()
        })
      })

      const result = await iii.trigger({
        function_id: 'test.readall.processor',
        payload: { reader: channel.readerRef },
      })

      await writePromise
      return result
    })

    await sleep(300)

    try {
      // biome-ignore lint/suspicious/noExplicitAny: test code
      const result = await iii.trigger<{ text: string }, any>({
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

      const result = await iii.trigger({
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
      const result = await iii.trigger<{ numbers: number[] }, any>({
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
