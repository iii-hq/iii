import { describe, expect, it } from 'vitest'
import { registerTriggerType } from '../../src/helpers'
import { iii, sleep } from './utils'

type TriggerTypeRow = {
  id: string
  worker_name: string
  description: string
}

describe('Trigger Types', () => {
  // `engine::trigger-types::list` was retired; the catalog now lives at
  // `engine::triggers::list` (which returns trigger TYPES, not instances).
  it('should list trigger types via engine::triggers::list', async () => {
    const { triggers } = await iii.trigger<
      { include_internal: boolean },
      { triggers: TriggerTypeRow[] }
    >({
      function_id: 'engine::triggers::list',
      payload: { include_internal: false },
    })
    expect(Array.isArray(triggers)).toBe(true)

    const httpType = triggers.find((tt) => tt.id === 'http')
    expect(httpType).toBeDefined()
    expect(httpType?.description).toBeDefined()
    expect(typeof httpType?.worker_name).toBe('string')
  })

  it('should return a TriggerTypeRef from registerTriggerType', async () => {
    const ref = registerTriggerType(
      iii,
      { id: 'browser.test.trigger-type', description: 'Browser test trigger type' },
      {
        async registerTrigger() {},
        async unregisterTrigger() {},
      },
    )

    expect(ref).toBeDefined()
    expect(ref.id).toBe('browser.test.trigger-type')
    expect(typeof ref.registerTrigger).toBe('function')
    expect(typeof ref.registerFunction).toBe('function')
    expect(typeof ref.unregister).toBe('function')

    ref.unregister()
  })

  it('should register a trigger via TriggerTypeRef', async () => {
    const ref = registerTriggerType(
      iii,
      { id: 'browser.test.tt-trigger', description: 'TT trigger test' },
      {
        async registerTrigger() {},
        async unregisterTrigger() {},
      },
    )

    const fn = iii.registerFunction('browser.test.tt-trigger.fn', async () => ({ ok: true }))

    await sleep(300)

    const trigger = ref.registerTrigger('browser.test.tt-trigger.fn', { key: 'value' })
    expect(trigger).toBeDefined()
    expect(typeof trigger.unregister).toBe('function')

    trigger.unregister()
    fn.unregister()
    ref.unregister()
  })

  it('should register a function with trigger via TriggerTypeRef', async () => {
    const ref = registerTriggerType(
      iii,
      { id: 'browser.test.tt-fn', description: 'TT function test' },
      {
        async registerTrigger() {},
        async unregisterTrigger() {},
      },
    )

    const fnRef = ref.registerFunction('browser.test.tt-fn.handler', async () => ({ ok: true }), {
      path: '/test',
    })

    expect(fnRef).toBeDefined()
    expect(fnRef.id).toBe('browser.test.tt-fn.handler')

    await sleep(300)

    const result = await iii.trigger<Record<string, never>, { ok: boolean }>({
      function_id: 'browser.test.tt-fn.handler',
      payload: {},
    })
    expect(result.ok).toBe(true)

    fnRef.unregister()
    ref.unregister()
  })

  it('should unregister a trigger type via TriggerTypeRef', () => {
    const ref = registerTriggerType(
      iii,
      { id: 'browser.test.tt-unreg', description: 'TT unregister test' },
      {
        async registerTrigger() {},
        async unregisterTrigger() {},
      },
    )

    expect(() => ref.unregister()).not.toThrow()
  })

  it('should accept include_internal parameter for engine::triggers::list', async () => {
    const { triggers: publicTypes } = await iii.trigger<
      { include_internal: boolean },
      { triggers: TriggerTypeRow[] }
    >({
      function_id: 'engine::triggers::list',
      payload: { include_internal: false },
    })
    const { triggers: allTypes } = await iii.trigger<
      { include_internal: boolean },
      { triggers: TriggerTypeRow[] }
    >({
      function_id: 'engine::triggers::list',
      payload: { include_internal: true },
    })

    expect(Array.isArray(publicTypes)).toBe(true)
    expect(Array.isArray(allTypes)).toBe(true)
    expect(allTypes.length).toBeGreaterThanOrEqual(publicTypes.length)
  })
})
