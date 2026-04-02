import { step } from '../src/step'
import { cron, http, queue, state, stream } from '../src/triggers'

describe('step', () => {
  const config = {
    name: 'test-step',
    triggers: [http('GET', '/test')],
  }

  it('step(config, handler) returns { config, handler }', () => {
    const handler = jest.fn()
    const result = step(config, handler)
    expect(result).toEqual({ config, handler })
  })

  it('step(config) returns builder with .handle method', () => {
    const result = step(config)
    expect(result.config).toEqual(config)
    expect(typeof result.handle).toBe('function')
  })

  it('builder .handle() returns { config, handler }', () => {
    const handler = jest.fn()
    const builder = step(config)
    const result = builder.handle(handler)
    expect(result.config).toEqual(config)
    expect(result.handler).toBe(handler)
  })

  it('creates step definition with state trigger', () => {
    const stateConfig = {
      name: 'state-step',
      triggers: [state()],
    }
    const handler = jest.fn()
    const result = step(stateConfig).handle(handler)

    expect(result.config.name).toBe('state-step')
    expect(result.config.triggers[0].type).toBe('state')
    expect(typeof result.handler).toBe('function')
  })

  it('creates step definition with stream trigger', () => {
    const streamConfig = {
      name: 'stream-step',
      triggers: [stream('events')],
    }
    const handler = jest.fn()
    const result = step(streamConfig).handle(handler)

    expect(result.config.name).toBe('stream-step')
    expect(result.config.triggers[0].type).toBe('stream')
    expect(typeof result.handler).toBe('function')
  })

  it('creates step definition with mixed triggers including state and stream', () => {
    const mixedConfig = {
      name: 'mixed-step',
      triggers: [queue('tasks'), state(), stream('events'), cron('0 * * * *')],
    }
    const result = step(mixedConfig).handle(jest.fn())

    expect(result.config.triggers).toHaveLength(4)
    expect(result.config.triggers.map((t: any) => t.type)).toEqual(['queue', 'state', 'stream', 'cron'])
  })
})
