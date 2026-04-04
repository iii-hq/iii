import { Stream } from '../src/new/stream'

const mockTrigger = jest.fn()

jest.mock('../src/new/iii', () => ({
  getInstance: () => ({ trigger: mockTrigger }),
}))

describe('Stream', () => {
  const config = {
    name: 'test-stream',
    schema: { type: 'object' },
    baseConfig: { storageType: 'default' as const },
  }

  let stream: Stream<typeof config>

  beforeEach(() => {
    mockTrigger.mockClear()
    stream = new Stream(config as any)
  })

  it('get() calls stream::get with stream_name, group_id, item_id', async () => {
    mockTrigger.mockResolvedValue({ id: '1', value: 'data' })
    await stream.get('group1', 'item1')
    expect(mockTrigger).toHaveBeenCalledWith({
      function_id: 'stream::get',
      payload: { stream_name: 'test-stream', group_id: 'group1', item_id: 'item1' },
    })
  })

  it('set() calls stream::set with full payload', async () => {
    mockTrigger.mockResolvedValue(null)
    await stream.set('group1', 'item1', { id: '1', value: 'x' } as any)
    expect(mockTrigger).toHaveBeenCalledWith({
      function_id: 'stream::set',
      payload: { stream_name: 'test-stream', group_id: 'group1', item_id: 'item1', data: { id: '1', value: 'x' } },
    })
  })

  it('delete() calls stream::delete', async () => {
    mockTrigger.mockResolvedValue(undefined)
    await stream.delete('group1', 'item1')
    expect(mockTrigger).toHaveBeenCalledWith({
      function_id: 'stream::delete',
      payload: { stream_name: 'test-stream', group_id: 'group1', item_id: 'item1' },
    })
  })

  it('list() calls stream::list', async () => {
    mockTrigger.mockResolvedValue([])
    await stream.list('group1')
    expect(mockTrigger).toHaveBeenCalledWith({
      function_id: 'stream::list',
      payload: { stream_name: 'test-stream', group_id: 'group1' },
    })
  })

  it('update() calls stream::update with ops', async () => {
    mockTrigger.mockResolvedValue(null)
    await stream.update('group1', 'item1', [{ type: 'set' as const, path: 'x', value: 1 }])
    expect(mockTrigger).toHaveBeenCalledWith({
      function_id: 'stream::update',
      payload: {
        stream_name: 'test-stream',
        group_id: 'group1',
        item_id: 'item1',
        ops: [{ type: 'set', path: 'x', value: 1 }],
      },
    })
  })

  it('listGroups() calls stream::list_groups', async () => {
    mockTrigger.mockResolvedValue(['g1', 'g2'])
    await stream.listGroups()
    expect(mockTrigger).toHaveBeenCalledWith({
      function_id: 'stream::list_groups',
      payload: { stream_name: 'test-stream' },
    })
  })

  it('send() calls stream::send with channel and event', async () => {
    mockTrigger.mockResolvedValue(undefined)
    await stream.send({ groupId: 'game', id: 'item1' }, { type: 'on-access-requested', data: { user: 'alice' } })
    expect(mockTrigger).toHaveBeenCalledWith({
      function_id: 'stream::send',
      payload: {
        stream_name: 'test-stream',
        group_id: 'game',
        id: 'item1',
        type: 'on-access-requested',
        data: { user: 'alice' },
      },
    })
  })
})
