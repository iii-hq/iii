import { multiTriggerStep } from '../src/multi-trigger'
import { cron, http, queue, state, stream } from '../src/triggers'

describe('multiTriggerStep', () => {
  const config = {
    name: 'multi-step',
    triggers: [queue('tasks'), http('GET', '/test'), cron('0 * * * *')],
  }

  it('returns builder with onQueue, onHttp, onCron, onState, onStream methods', () => {
    const builder = multiTriggerStep(config)
    expect(builder.config).toEqual(config)
    expect(typeof builder.onQueue).toBe('function')
    expect(typeof builder.onHttp).toBe('function')
    expect(typeof builder.onCron).toBe('function')
    expect(typeof builder.onState).toBe('function')
    expect(typeof builder.onStream).toBe('function')
    expect(typeof builder.handlers).toBe('function')
  })

  it('onQueue(handler).handlers() routes queue triggers to queue handler', async () => {
    const queueHandler = jest.fn().mockResolvedValue(undefined)
    const { handler } = multiTriggerStep(config).onQueue(queueHandler).handlers()
    const ctx = { trigger: { type: 'queue' }, logger: { warn: jest.fn() } }
    await handler({ data: 'test' }, ctx)
    expect(queueHandler).toHaveBeenCalledWith({ data: 'test' }, ctx)
  })

  it('onHttp(handler).handlers() routes http triggers to http handler', async () => {
    const httpHandler = jest.fn().mockResolvedValue({ status: 200, body: null })
    const { handler } = multiTriggerStep(config).onHttp(httpHandler).handlers()
    const ctx = { trigger: { type: 'http' }, logger: { warn: jest.fn() } }
    const req = { body: {}, pathParams: {}, queryParams: {}, headers: {} }
    await handler(req, ctx)
    expect(httpHandler).toHaveBeenCalledWith(req, ctx)
  })

  it('onCron(handler).handlers() routes cron triggers to cron handler', async () => {
    const cronHandler = jest.fn().mockResolvedValue(undefined)
    const { handler } = multiTriggerStep(config).onCron(cronHandler).handlers()
    const ctx = { trigger: { type: 'cron' }, logger: { warn: jest.fn() } }
    await handler(undefined, ctx)
    expect(cronHandler).toHaveBeenCalledWith(ctx)
  })

  it('onState(handler).handlers() routes state triggers to state handler', async () => {
    const stateHandler = jest.fn().mockResolvedValue(undefined)
    const { handler } = multiTriggerStep(config).onState(stateHandler).handlers()
    const ctx = { trigger: { type: 'state' }, logger: { warn: jest.fn() } }
    const input = { type: 'state', group_id: 'g1', item_id: 'i1', new_value: 42 }
    await handler(input, ctx)
    expect(stateHandler).toHaveBeenCalledWith(input, ctx)
  })

  it('onStream(handler).handlers() routes stream triggers to stream handler', async () => {
    const streamHandler = jest.fn().mockResolvedValue(undefined)
    const { handler } = multiTriggerStep(config).onStream(streamHandler).handlers()
    const ctx = { trigger: { type: 'stream' }, logger: { warn: jest.fn() } }
    const input = {
      type: 'stream',
      timestamp: 123,
      streamName: 'events',
      groupId: 'g1',
      id: '1',
      event: { type: 'create', data: {} },
    }
    await handler(input, ctx)
    expect(streamHandler).toHaveBeenCalledWith(input, ctx)
  })

  it('handlers({ state, stream }) accepts state/stream via dict', async () => {
    const stateHandler = jest.fn().mockResolvedValue(undefined)
    const streamHandler = jest.fn().mockResolvedValue(undefined)
    const { handler } = multiTriggerStep(config).handlers({ state: stateHandler, stream: streamHandler })

    await handler({ type: 'state', group_id: 'g1', item_id: 'i1' } as any, {
      trigger: { type: 'state' },
      logger: { warn: jest.fn() },
    })
    expect(stateHandler).toHaveBeenCalled()

    await handler(
      {
        type: 'stream',
        streamName: 'e',
        groupId: 'g',
        id: '1',
        timestamp: 0,
        event: { type: 'create', data: {} },
      } as any,
      { trigger: { type: 'stream' }, logger: { warn: jest.fn() } },
    )
    expect(streamHandler).toHaveBeenCalled()
  })

  it('handlers({ queue, http }) accepts all at once', async () => {
    const queueHandler = jest.fn().mockResolvedValue(undefined)
    const httpHandler = jest.fn().mockResolvedValue({ status: 200, body: null })
    const { handler } = multiTriggerStep(config).handlers({ queue: queueHandler, http: httpHandler })

    await handler({ data: 'q' }, { trigger: { type: 'queue' }, logger: { warn: jest.fn() } })
    expect(queueHandler).toHaveBeenCalled()

    await handler({ body: {} } as any, { trigger: { type: 'http' }, logger: { warn: jest.fn() } })
    expect(httpHandler).toHaveBeenCalled()
  })

  it('unified handler throws when no handler matches trigger type', async () => {
    const logger = { warn: jest.fn() }
    const { handler } = multiTriggerStep(config).onQueue(jest.fn()).handlers()
    const ctx = { trigger: { type: 'http' }, logger }

    await expect(handler({} as any, ctx)).rejects.toThrow(
      'No handler defined for trigger type: http. Available handlers: queue',
    )
  })

  it('unified handler logs warning before throwing', async () => {
    const logger = { warn: jest.fn() }
    const { handler } = multiTriggerStep(config).onQueue(jest.fn()).handlers()
    const ctx = { trigger: { type: 'cron' }, logger }

    await expect(handler(undefined, ctx)).rejects.toThrow()
    expect(logger.warn).toHaveBeenCalledWith('No handler defined for trigger type: cron', {
      availableHandlers: ['queue'],
      triggerType: 'cron',
    })
  })
})
