import { StateManager } from '../src/new/state'

const mockTrigger = jest.fn()

jest.mock('../src/new/iii', () => ({
  getInstance: () => ({ trigger: mockTrigger }),
}))

describe('StateManager', () => {
  let manager: StateManager

  beforeEach(() => {
    mockTrigger.mockClear()
    manager = new StateManager()
  })

  it('get() calls state::get with scope and key', async () => {
    mockTrigger.mockResolvedValue({ id: '1', value: 'data' })
    await manager.get('scope1', 'key1')
    expect(mockTrigger).toHaveBeenCalledWith({
      function_id: 'state::get',
      payload: { scope: 'scope1', key: 'key1' },
    })
  })

  it('set() calls state::set with scope, key, value', async () => {
    mockTrigger.mockResolvedValue(null)
    await manager.set('scope1', 'key1', { name: 'test' })
    expect(mockTrigger).toHaveBeenCalledWith({
      function_id: 'state::set',
      payload: { scope: 'scope1', key: 'key1', value: { name: 'test' } },
    })
  })

  it('delete() calls state::delete with scope and key', async () => {
    mockTrigger.mockResolvedValue(null)
    await manager.delete('scope1', 'key1')
    expect(mockTrigger).toHaveBeenCalledWith({
      function_id: 'state::delete',
      payload: { scope: 'scope1', key: 'key1' },
    })
  })

  it('list() calls state::list with scope', async () => {
    mockTrigger.mockResolvedValue([])
    await manager.list('scope1')
    expect(mockTrigger).toHaveBeenCalledWith({
      function_id: 'state::list',
      payload: { scope: 'scope1' },
    })
  })

  it('listGroups() calls state::list_groups with empty object', async () => {
    mockTrigger.mockResolvedValue([])
    await manager.listGroups()
    expect(mockTrigger).toHaveBeenCalledWith({
      function_id: 'state::list_groups',
      payload: {},
    })
  })

  it('update() calls state::update with scope, key, ops', async () => {
    mockTrigger.mockResolvedValue(null)
    await manager.update('scope1', 'key1', [{ type: 'set' as const, path: 'x', value: 1 }])
    expect(mockTrigger).toHaveBeenCalledWith({
      function_id: 'state::update',
      payload: { scope: 'scope1', key: 'key1', ops: [{ type: 'set', path: 'x', value: 1 }] },
    })
  })

  it('clear() lists then deletes each item', async () => {
    const clearScope = 'clearScope'
    mockTrigger.mockResolvedValueOnce([{ id: 'a' }, { id: 'b' }]).mockResolvedValue(null)
    await manager.clear(clearScope)
    expect(mockTrigger).toHaveBeenCalledWith({
      function_id: 'state::list',
      payload: { scope: clearScope },
    })
    expect(mockTrigger).toHaveBeenCalledWith({
      function_id: 'state::delete',
      payload: { scope: clearScope, key: 'a' },
    })
    expect(mockTrigger).toHaveBeenCalledWith({
      function_id: 'state::delete',
      payload: { scope: clearScope, key: 'b' },
    })
  })
})
