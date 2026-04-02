import { Motia } from '../src/new/build/utils'
import { cron, http, queue, state, stream } from '../src/triggers'

const mockRegisterFunction = jest.fn()
const mockRegisterTrigger = jest.fn()
const mockCall = jest.fn()
jest.mock('../src/new/iii', () => ({
  getInstance: () => ({
    call: mockCall,
    registerFunction: mockRegisterFunction,
    registerTrigger: mockRegisterTrigger,
  }),
}))

jest.mock('iii-sdk', () => ({
  Logger: function MockLogger() {
    return { warn: jest.fn(), info: jest.fn(), error: jest.fn(), debug: jest.fn() }
  },
  http: (callback: (...args: unknown[]) => unknown) => callback,
}))

jest.mock('iii-sdk/telemetry', () => ({
  currentTraceId: () => 'test-trace-id',
}))

jest.mock('../src/new/setup-step-endpoint', () => ({
  setupStepEndpoint: jest.fn(),
}))

describe('Motia', () => {
  beforeEach(() => {
    jest.clearAllMocks()
  })

  describe('addStep', () => {
    it('registers function and trigger for http trigger', () => {
      const motia = new Motia()
      const config = { name: 'test', triggers: [http('GET', '/users')] }
      const handler = jest.fn().mockResolvedValue({ status: 200, body: null })

      motia.addStep(config as any, 'test.step.ts', handler, 'steps/test.step.ts')

      expect(mockRegisterFunction).toHaveBeenCalled()
      expect(mockRegisterTrigger).toHaveBeenCalledWith(
        expect.objectContaining({
          type: 'http',
          config: expect.objectContaining({
            api_path: 'users',
            http_method: 'GET',
          }),
        }),
      )
    })

    it('strips leading slash from API path', () => {
      const motia = new Motia()
      const config = { name: 'test', triggers: [http('POST', '/api/items')] }

      motia.addStep(config as any, 'test.step.ts', jest.fn(), 'steps/test.step.ts')

      const triggerCall = mockRegisterTrigger.mock.calls[0][0]
      expect(triggerCall.config.api_path).toBe('api/items')
    })

    it('registers condition function when trigger has condition', () => {
      const motia = new Motia()
      const condition = jest.fn().mockResolvedValue(true)
      const config = { name: 'test', triggers: [http('GET', '/x', undefined, condition)] }

      motia.addStep(config as any, 'test.step.ts', jest.fn(), 'steps/test.step.ts')

      expect(mockRegisterFunction).toHaveBeenCalledTimes(2)
      expect(mockRegisterTrigger.mock.calls[0][0].config.condition_function_id).toBeDefined()
    })

    it('registers function and trigger for queue trigger', () => {
      const motia = new Motia()
      const config = { name: 'test', triggers: [queue('tasks')] }

      motia.addStep(config as any, 'test.step.ts', jest.fn(), 'steps/test.step.ts')

      expect(mockRegisterTrigger).toHaveBeenCalledWith(
        expect.objectContaining({
          type: 'queue',
          config: expect.objectContaining({ topic: 'tasks' }),
        }),
      )
    })

    it('passes queue config in queue trigger', () => {
      const motia = new Motia()
      const config = {
        name: 'test',
        triggers: [queue('tasks', { config: { maxRetries: 5, type: 'fifo' } })],
      }
      motia.addStep(config as any, 'test.step.ts', jest.fn(), 'steps/test.step.ts')

      const triggerCall = mockRegisterTrigger.mock.calls[0][0]
      expect(triggerCall.config.queue_config).toEqual({
        maxRetries: 5,
        type: 'fifo',
      })
    })

    it('omits queue_config when not provided', () => {
      const motia = new Motia()
      const config = { name: 'test', triggers: [queue('tasks')] }
      motia.addStep(config as any, 'test.step.ts', jest.fn(), 'steps/test.step.ts')

      const triggerCall = mockRegisterTrigger.mock.calls[0][0]
      expect(triggerCall.config.queue_config).toBeUndefined()
    })

    it('registers function and trigger for state trigger', () => {
      const motia = new Motia()
      const config = { name: 'test', triggers: [state()] }

      motia.addStep(config as any, 'test.step.ts', jest.fn(), 'steps/test.step.ts')

      expect(mockRegisterTrigger).toHaveBeenCalledWith(
        expect.objectContaining({
          type: 'state',
        }),
      )
    })

    it('registers function and trigger for stream trigger', () => {
      const motia = new Motia()
      const config = { name: 'test', triggers: [stream('events')] }

      motia.addStep(config as any, 'test.step.ts', jest.fn(), 'steps/test.step.ts')

      expect(mockRegisterTrigger).toHaveBeenCalledWith(
        expect.objectContaining({
          type: 'stream',
          config: expect.objectContaining({ stream_name: 'events' }),
        }),
      )
    })

    it('registers function and trigger for cron trigger', () => {
      const motia = new Motia()
      const config = { name: 'test', triggers: [cron('0 * * * *')] }

      motia.addStep(config as any, 'test.step.ts', jest.fn(), 'steps/test.step.ts')

      expect(mockRegisterTrigger).toHaveBeenCalledWith(
        expect.objectContaining({
          type: 'cron',
          config: expect.objectContaining({ expression: '0 * * * *' }),
        }),
      )
    })

    it('registers condition function when cron trigger has condition', () => {
      const motia = new Motia()
      const condition = jest.fn().mockResolvedValue(true)
      const config = { name: 'test', triggers: [cron('0 0 * * *', condition)] }

      motia.addStep(config as any, 'test.step.ts', jest.fn(), 'steps/test.step.ts')

      expect(mockRegisterFunction).toHaveBeenCalledTimes(2)
      expect(mockRegisterTrigger.mock.calls[0][0].config.condition_function_id).toBeDefined()
    })
  })

  describe('middleware composition', () => {
    it('executes middleware around handler for http trigger', async () => {
      const motia = new Motia()
      const order: number[] = []

      const middleware = async (_req: any, _ctx: any, next: () => Promise<any>) => {
        order.push(1)
        const result = await next()
        order.push(3)
        return result
      }

      const handler = async (_req: any, _ctx: any) => {
        order.push(2)
        return { status: 200, body: null }
      }

      const config = { name: 'test', triggers: [http('GET', '/test', { middleware: [middleware] })] }
      motia.addStep(config as any, 'test.step.ts', handler, 'steps/test.step.ts')

      const registeredHandler = mockRegisterFunction.mock.calls[0][1]
      await registeredHandler({ body: {}, path_params: {}, query_params: {}, headers: {} })

      expect(order).toEqual([1, 2, 3])
    })
  })

  describe('addStream', () => {
    it('adds stream to streams record', () => {
      const motia = new Motia()
      const config = {
        name: 'users',
        schema: { type: 'object' },
        baseConfig: { storageType: 'default' as const },
      }

      motia.addStream(config as any, 'users.stream.ts')

      expect(motia.streams.users).toBeDefined()
      expect(motia.streams.users.config.name).toBe('users')
    })
  })

  describe('initialize', () => {
    it('calls setupStepEndpoint', () => {
      const { setupStepEndpoint } = require('../src/new/setup-step-endpoint')
      const motia = new Motia()

      motia.initialize()

      expect(setupStepEndpoint).toHaveBeenCalled()
    })
  })

  describe('flowContext via handler invocation', () => {
    it('handler receives context with correct is checks and match routing', async () => {
      const motia = new Motia()
      const config = { name: 'test', triggers: [queue('q')] }
      let capturedContext: any

      const handler = async (_input: unknown, ctx: any) => {
        capturedContext = ctx
        expect(ctx.is.queue(_input)).toBe(true)
        expect(ctx.is.http(_input)).toBe(false)
        const result = await ctx.match({
          queue: async () => 'queue-result',
        })
        return result
      }

      motia.addStep(config as any, 'test.step.ts', handler, 'steps/test.step.ts')

      const registeredHandler = mockRegisterFunction.mock.calls[0][1]
      const result = await registeredHandler({ data: 'test' })

      expect(capturedContext.traceId).toBe('test-trace-id')
      expect(capturedContext.trigger.type).toBe('queue')
    })

    it('getData returns body for http trigger', async () => {
      const motia = new Motia()
      const config = { name: 'test', triggers: [http('POST', '/x')] }
      let capturedContext: any

      const handler = async (_req: any, ctx: any) => {
        capturedContext = ctx
        return { status: 200, body: null }
      }

      motia.addStep(config as any, 'test.step.ts', handler, 'steps/test.step.ts')

      const registeredHandler = mockRegisterFunction.mock.calls[0][1]
      await registeredHandler({
        path_params: {},
        query_params: {},
        body: { foo: 'bar' },
        headers: {},
      })

      expect(capturedContext.getData()).toEqual({ foo: 'bar' })
    })

    it('match throws when no handler matches', async () => {
      const motia = new Motia()
      const config = { name: 'test', triggers: [queue('q')] }

      const handler = async (_input: unknown, ctx: any): Promise<any> => {
        await ctx.match({})
      }

      motia.addStep(config as any, 'test.step.ts', handler, 'steps/test.step.ts')

      const registeredHandler = mockRegisterFunction.mock.calls[0][1]

      await expect(registeredHandler({ data: 'test' })).rejects.toThrow('No handler matched for trigger type: queue')
    })

    it('is.state returns true for state trigger', async () => {
      const motia = new Motia()
      const config = { name: 'test', triggers: [state()] }

      const handler = async (input: unknown, ctx: any): Promise<any> => {
        expect(ctx.is.state(input)).toBe(true)
        expect(ctx.is.queue(input)).toBe(false)
        expect(ctx.is.http(input)).toBe(false)
        expect(ctx.is.stream(input)).toBe(false)
      }

      motia.addStep(config as any, 'test.step.ts', handler, 'steps/test.step.ts')
      const registeredHandler = mockRegisterFunction.mock.calls[0][1]
      await registeredHandler({ type: 'state', group_id: 'g1', item_id: 'i1' })
    })

    it('is.stream returns true for stream trigger', async () => {
      const motia = new Motia()
      const config = { name: 'test', triggers: [stream('events')] }

      const handler = async (input: unknown, ctx: any): Promise<any> => {
        expect(ctx.is.stream(input)).toBe(true)
        expect(ctx.is.queue(input)).toBe(false)
        expect(ctx.is.state(input)).toBe(false)
      }

      motia.addStep(config as any, 'test.step.ts', handler, 'steps/test.step.ts')
      const registeredHandler = mockRegisterFunction.mock.calls[0][1]
      await registeredHandler({
        type: 'stream',
        streamName: 's1',
        groupId: 'g1',
        id: '1',
        timestamp: 0,
        event: { type: 'create', data: {} },
      })
    })

    it('match routes to state handler for state trigger', async () => {
      const motia = new Motia()
      const config = { name: 'test', triggers: [state()] }

      const handler = async (input: unknown, ctx: any): Promise<any> => {
        return ctx.match({
          state: async () => 'state-handled',
        })
      }

      motia.addStep(config as any, 'test.step.ts', handler, 'steps/test.step.ts')
      const registeredHandler = mockRegisterFunction.mock.calls[0][1]
      const result = await registeredHandler({ type: 'state', group_id: 'g1', item_id: 'i1' })
      expect(result).toBe('state-handled')
    })

    it('getData returns input for state trigger', async () => {
      const motia = new Motia()
      const config = { name: 'test', triggers: [state()] }
      const stateInput = { type: 'state', group_id: 'g1', item_id: 'i1' }
      let capturedData: unknown

      const handler = async (_input: unknown, ctx: any) => {
        capturedData = ctx.getData()
        return undefined
      }

      motia.addStep(config as any, 'test.step.ts', handler, 'steps/test.step.ts')
      const registeredHandler = mockRegisterFunction.mock.calls[0][1]
      await registeredHandler(stateInput)

      expect(capturedData).toEqual(stateInput)
    })

    it('getData returns input for stream trigger', async () => {
      const motia = new Motia()
      const config = { name: 'test', triggers: [stream('events')] }
      const streamInput = {
        type: 'stream',
        streamName: 's1',
        groupId: 'g1',
        id: '1',
        timestamp: 0,
        event: { type: 'create', data: { foo: 'bar' } },
      }
      let capturedData: unknown

      const handler = async (_input: unknown, ctx: any) => {
        capturedData = ctx.getData()
        return undefined
      }

      motia.addStep(config as any, 'test.step.ts', handler, 'steps/test.step.ts')
      const registeredHandler = mockRegisterFunction.mock.calls[0][1]
      await registeredHandler(streamInput)

      expect(capturedData).toEqual(streamInput)
    })

    it('match passes input to state handler for state trigger', async () => {
      const motia = new Motia()
      const config = { name: 'test', triggers: [state()] }
      const stateInput = { type: 'state', group_id: 'g1', item_id: 'i1' }
      let receivedInput: unknown

      const handler = async (_input: unknown, ctx: any): Promise<any> => {
        return ctx.match({
          state: async (input) => {
            receivedInput = input
            return 'state-handled'
          },
        })
      }

      motia.addStep(config as any, 'test.step.ts', handler, 'steps/test.step.ts')
      const registeredHandler = mockRegisterFunction.mock.calls[0][1]
      const result = await registeredHandler(stateInput)

      expect(result).toBe('state-handled')
      expect(receivedInput).toEqual(stateInput)
    })

    it('match passes input to stream handler for stream trigger', async () => {
      const motia = new Motia()
      const config = { name: 'test', triggers: [stream('events')] }
      const streamInput = {
        type: 'stream',
        streamName: 's1',
        groupId: 'g1',
        id: '1',
        timestamp: 0,
        event: { type: 'create', data: {} },
      }
      let receivedInput: unknown

      const handler = async (_input: unknown, ctx: any): Promise<any> => {
        return ctx.match({
          stream: async (input) => {
            receivedInput = input
            return 'stream-handled'
          },
        })
      }

      motia.addStep(config as any, 'test.step.ts', handler, 'steps/test.step.ts')
      const registeredHandler = mockRegisterFunction.mock.calls[0][1]
      const result = await registeredHandler(streamInput)

      expect(result).toBe('stream-handled')
      expect(receivedInput).toEqual(streamInput)
    })

    it('match uses default handler when no specific handler matches', async () => {
      const motia = new Motia()
      const config = { name: 'test', triggers: [queue('q')] }

      const handler = async (input: unknown, ctx: any): Promise<any> => {
        return ctx.match({
          default: async (inp: any) => 'default-result',
        })
      }

      motia.addStep(config as any, 'test.step.ts', handler, 'steps/test.step.ts')
      const registeredHandler = mockRegisterFunction.mock.calls[0][1]
      const result = await registeredHandler({ data: 'test' })
      expect(result).toBe('default-result')
    })
  })
})
