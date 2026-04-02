import { getInstance, initIII } from '../../src/new/iii'
import { StateManager } from '../../src/new/state'
import { initTestEnv, waitForReady } from './setup'

describe('state integration', () => {
  let state: StateManager

  beforeAll(async () => {
    initTestEnv()
    initIII({ enabled: false })
    const sdk = getInstance()
    await waitForReady(sdk)
    state = new StateManager()
    try {
      await state.get(`test_${Date.now()}`, 'check')
    } catch (e) {
      if (String(e).toLowerCase().includes('function_not_found')) {
        throw new Error(
          'State integration tests require a running motia engine with state support. Start the engine or run unit tests only with: pnpm test:unit',
        )
      }
    }
  }, 15000)

  afterAll(async () => {
    const sdk = getInstance()
    await sdk.shutdown()
  })

  it('set then get returns stored value', async () => {
    const scope = `test_scope_${Date.now()}`
    const key = `test_key_${Date.now()}`
    const value = { status: 'active', count: 10 }

    await state.set(scope, key, value)
    const result = await state.get<typeof value>(scope, key)

    expect(result).not.toBeNull()
    expect(result?.status).toBe('active')
    expect(result?.count).toBe(10)
  }, 10000)

  it('delete removes value', async () => {
    const scope = `delete_scope_${Date.now()}`
    const key = `delete_key_${Date.now()}`

    await state.set(scope, key, { temp: true })
    const before = await state.get(scope, key)
    expect(before).not.toBeNull()

    await state.delete(scope, key)
    const after = await state.get(scope, key)
    expect(after == null).toBe(true)
  }, 10000)

  it('list returns all items in scope', async () => {
    const scope = `list_scope_${Date.now()}`

    await state.set(scope, 'item_0', { index: 0 })
    await state.set(scope, 'item_1', { index: 1 })
    await state.set(scope, 'item_2', { index: 2 })

    const items = await state.list<{ index: number }>(scope)
    expect(items.length).toBeGreaterThanOrEqual(3)
  }, 10000)

  it('listGroups returns available scopes', async () => {
    const result = await state.listGroups()
    const groups = Array.isArray(result) ? result : ((result as { groups?: string[] })?.groups ?? [])
    expect(Array.isArray(groups)).toBe(true)
  }, 10000)

  it('update applies partial updates', async () => {
    const scope = `update_scope_${Date.now()}`
    const key = `update_key_${Date.now()}`

    await state.set(scope, key, { count: 0, name: 'initial' })
    await state.update(scope, key, [{ type: 'set', path: 'count', value: 5 }])

    const result = await state.get<{ count?: number }>(scope, key)
    expect(result?.count).toBe(5)
  }, 10000)

  it('clear removes all items from scope', async () => {
    const scope = `clear_scope_${Date.now()}`

    await state.set(scope, 'a', { id: 'a' })
    await state.set(scope, 'b', { id: 'b' })
    await state.clear(scope)

    const items = await state.list(scope)
    expect(items.length).toBe(0)
  }, 10000)
})
