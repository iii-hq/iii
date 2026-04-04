import { describe, expect, it } from 'vitest'
import { iii, sleep } from './utils'

describe('Trigger Registration', () => {
  it('should register and list a trigger', async () => {
    const fn = iii.registerFunction('browser.test.triggers.fn', async () => ({ ok: true }))

    const trigger = iii.registerTrigger({
      type: 'http',
      function_id: fn.id,
      config: { api_path: 'browser-test/triggers', http_method: 'GET' },
    })

    await sleep(500)

    const triggers = await iii.listTriggers()
    expect(Array.isArray(triggers)).toBe(true)

    const found = triggers.find((t) => t.function_id === 'browser.test.triggers.fn')
    expect(found).toBeDefined()
    expect(found?.trigger_type).toBe('http')

    trigger.unregister()
    fn.unregister()
  })

  it('should unregister a trigger', async () => {
    const fn = iii.registerFunction('browser.test.triggers.unreg', async () => ({ ok: true }))

    const trigger = iii.registerTrigger({
      type: 'http',
      function_id: fn.id,
      config: { api_path: 'browser-test/triggers-unreg', http_method: 'GET' },
    })

    await sleep(500)

    trigger.unregister()
    await sleep(300)

    const triggers = await iii.listTriggers()
    const found = triggers.find((t) => t.function_id === 'browser.test.triggers.unreg')
    expect(found).toBeUndefined()

    fn.unregister()
  })

  it('should return triggers as an array even when empty', async () => {
    const triggers = await iii.listTriggers()
    expect(Array.isArray(triggers)).toBe(true)
  })

  it('should support includeInternal parameter', async () => {
    const triggers = await iii.listTriggers(false)
    expect(Array.isArray(triggers)).toBe(true)
  })

  it('should register multiple triggers for the same function', async () => {
    const fn = iii.registerFunction('browser.test.triggers.multi', async () => ({ ok: true }))

    const trigger1 = iii.registerTrigger({
      type: 'http',
      function_id: fn.id,
      config: { api_path: 'browser-test/multi-1', http_method: 'GET' },
    })

    const trigger2 = iii.registerTrigger({
      type: 'http',
      function_id: fn.id,
      config: { api_path: 'browser-test/multi-2', http_method: 'POST' },
    })

    await sleep(500)

    const triggers = await iii.listTriggers()
    const found = triggers.filter((t) => t.function_id === 'browser.test.triggers.multi')
    expect(found.length).toBeGreaterThanOrEqual(2)

    trigger1.unregister()
    trigger2.unregister()
    fn.unregister()
  })
})
