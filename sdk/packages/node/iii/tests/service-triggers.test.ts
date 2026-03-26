import { describe, expect, it } from 'vitest'
import type { ChannelReader } from '../src'
import { iii, sleep } from './utils'

describe('Service Registration', () => {
  it('should register a service with just an id', async () => {
    iii.registerService({ id: 'test.svc.basic' })
    await sleep(300)

    const fn = iii.registerFunction({ id: 'test.svc.basic.hello' }, async () => ({ ok: true }))

    await sleep(300)

    const result = await iii.trigger<Record<string, never>, { ok: boolean }>({
      function_id: 'test.svc.basic.hello',
      payload: {},
    })
    expect(result.ok).toBe(true)

    fn.unregister()
  })

  it('should register a service with a custom name', async () => {
    iii.registerService({ id: 'test.svc.named', name: 'My Named Service' })
    await sleep(300)

    const fn = iii.registerFunction({ id: 'test.svc.named.ping' }, async () => ({ pong: true }))

    await sleep(300)

    const result = await iii.trigger<Record<string, never>, { pong: boolean }>({
      function_id: 'test.svc.named.ping',
      payload: {},
    })
    expect(result.pong).toBe(true)

    fn.unregister()
  })

  it('should register a service with description and parent', async () => {
    iii.registerService({ id: 'test.svc.parent' })
    iii.registerService({
      id: 'test.svc.child',
      description: 'A child service',
      parent_service_id: 'test.svc.parent',
    })

    await sleep(300)

    const fn = iii.registerFunction({ id: 'test.svc.child.action' }, async (data: { value: number }) => ({
      doubled: data.value * 2,
    }))

    await sleep(300)

    const result = await iii.trigger<{ value: number }, { doubled: number }>({
      function_id: 'test.svc.child.action',
      payload: { value: 5 },
    })
    expect(result.doubled).toBe(10)

    fn.unregister()
  })

  it('should default name to id when name is not provided', async () => {
    iii.registerService({ id: 'test.svc.default-name', description: 'No explicit name' })
    await sleep(300)

    const fn = iii.registerFunction({ id: 'test.svc.default-name.check' }, async () => ({ status: 'ok' }))

    await sleep(300)

    const result = await iii.trigger<Record<string, never>, { status: string }>({
      function_id: 'test.svc.default-name.check',
      payload: {},
    })
    expect(result.status).toBe('ok')

    fn.unregister()
  })
})

describe('List Triggers', () => {
  it('should list registered triggers', async () => {
    const fn = iii.registerFunction({ id: 'test.triggers.list.func' }, async () => ({ ok: true }))

    const trigger = iii.registerTrigger({
      type: 'http',
      function_id: fn.id,
      config: { api_path: 'test/list-triggers', http_method: 'GET' },
    })

    await sleep(500)

    const triggers = await iii.listTriggers()
    expect(Array.isArray(triggers)).toBe(true)

    const found = triggers.find((t) => t.function_id === 'test.triggers.list.func')
    expect(found).toBeDefined()
    expect(found?.trigger_type).toBe('http')

    trigger.unregister()
    fn.unregister()
  })

  it('should return an array even when no triggers exist', async () => {
    const triggers = await iii.listTriggers()
    expect(Array.isArray(triggers)).toBe(true)
  })

  it('should accept includeInternal parameter', async () => {
    const triggers = await iii.listTriggers(false)
    expect(Array.isArray(triggers)).toBe(true)
  })
})

describe('List Trigger Types', () => {
  it('should list registered trigger types', async () => {
    const triggerTypes = await iii.listTriggerTypes()
    expect(Array.isArray(triggerTypes)).toBe(true)

    // The engine always registers the built-in 'http' trigger type
    const httpType = triggerTypes.find((tt) => tt.id === 'http')
    expect(httpType).toBeDefined()
    expect(httpType?.description).toBeDefined()
  })

  it('should return an array even when called with no custom trigger types', async () => {
    const triggerTypes = await iii.listTriggerTypes()
    expect(Array.isArray(triggerTypes)).toBe(true)
  })

  it('should accept includeInternal parameter', async () => {
    const triggerTypes = await iii.listTriggerTypes(false)
    expect(Array.isArray(triggerTypes)).toBe(true)

    const triggerTypesWithInternal = await iii.listTriggerTypes(true)
    expect(Array.isArray(triggerTypesWithInternal)).toBe(true)
    expect(triggerTypesWithInternal.length).toBeGreaterThanOrEqual(triggerTypes.length)
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

    const fn = iii.registerFunction({ id: 'test.tt-ref-trigger.fn' }, async () => ({ ok: true }))

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
      { id: 'test.tt-ref-fn.handler' },
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
      { id: 'test.readall.processor' },
      async (input: { reader: ChannelReader }) => {
        const data = await input.reader.readAll()
        return { content: data.toString('utf-8'), size: data.length }
      },
    )

    const sender = iii.registerFunction({ id: 'test.readall.sender' }, async (input: { text: string }) => {
      const channel = await iii.createChannel()

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
      { id: 'test.readall.chunked.processor' },
      async (input: { reader: ChannelReader }) => {
        const data = await input.reader.readAll()
        const items = JSON.parse(data.toString('utf-8'))
        return { count: items.length, total: items.reduce((s: number, n: number) => s + n, 0) }
      },
    )

    const sender = iii.registerFunction({ id: 'test.readall.chunked.sender' }, async (input: { numbers: number[] }) => {
      const channel = await iii.createChannel()

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
