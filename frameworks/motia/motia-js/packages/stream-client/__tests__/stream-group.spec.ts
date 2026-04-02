import type { GroupEventMessage, JoinMessage } from '../src/stream.types'
import { StreamGroupSubscription } from '../src/stream-group'

type TestData = { id: string; name: string; value: number }

describe('StreamGroupSubscription', () => {
  const joinMessage: JoinMessage = {
    streamName: 'test-stream',
    groupId: 'test-group',
    subscriptionId: 'sub-1',
  }

  function makeMessage(
    type: GroupEventMessage<TestData>['event']['type'],
    data: unknown,
    timestamp = Date.now(),
  ): GroupEventMessage<TestData> {
    return {
      streamName: 'test-stream',
      groupId: 'test-group',
      timestamp,
      event: { type, ...(type === 'event' ? { event: data } : { data }) },
    } as GroupEventMessage<TestData>
  }

  it('notifies change listeners on state change', () => {
    const sub = new StreamGroupSubscription<TestData>(joinMessage)
    const listener = jest.fn()

    sub.addChangeListener(listener)
    const syncData = [
      { id: '1', name: 'A' },
      { id: '2', name: 'B' },
    ]
    sub.listener(makeMessage('sync', syncData))
    expect(listener).toHaveBeenCalledWith(syncData)
  })

  it('should be able to sort by a key', () => {
    const sub = new StreamGroupSubscription<TestData>(joinMessage, 'name')
    const listener = jest.fn()

    sub.addChangeListener(listener)
    const syncData = [
      { id: '1', name: 'B' },
      { id: '2', name: 'A' },
    ]
    sub.listener(makeMessage('sync', syncData))
    expect(listener).toHaveBeenCalledWith(syncData)

    expect(sub.getState()).toEqual([
      { id: '2', name: 'A' },
      { id: '1', name: 'B' },
    ])
  })

  it('should discard old events', () => {
    const sub = new StreamGroupSubscription<TestData>(joinMessage)
    const listener = jest.fn()

    sub.addChangeListener(listener)

    const syncData = [
      { id: '1', name: 'A' },
      { id: '2', name: 'B' },
    ]
    sub.listener(makeMessage('sync', syncData))

    const oldSyncData = [
      { id: '3', name: 'C' },
      { id: '4', name: 'D' },
    ]
    sub.listener(makeMessage('sync', oldSyncData, Date.now() - 1000))

    expect(sub.getState()).toEqual(syncData)
  })

  it('should add items', () => {
    const sub = new StreamGroupSubscription<TestData>(joinMessage)
    const syncData = [
      { id: '1', name: 'A' },
      { id: '2', name: 'B' },
    ]

    sub.listener(makeMessage('sync', syncData))
    sub.listener(makeMessage('create', { id: '3', name: 'C' }))

    expect(sub.getState()).toEqual([...syncData, { id: '3', name: 'C' }])
  })

  it('should update items', () => {
    const sub = new StreamGroupSubscription<TestData>(joinMessage)
    const syncData = [
      { id: '1', name: 'A' },
      { id: '2', name: 'B' },
    ]

    sub.listener(makeMessage('sync', syncData))
    sub.listener(makeMessage('update', { id: '1', name: 'A1' }))

    expect(sub.getState()).toEqual([
      { id: '1', name: 'A1' },
      { id: '2', name: 'B' },
    ])
  })

  it('should remove items', () => {
    const sub = new StreamGroupSubscription<TestData>(joinMessage)
    const syncData = [
      { id: '1', name: 'A' },
      { id: '2', name: 'B' },
    ]

    sub.listener(makeMessage('sync', syncData))
    sub.listener(makeMessage('delete', { id: '1' }))

    expect(sub.getState()).toEqual([{ id: '2', name: 'B' }])
  })

  it('should discard old on event on a particular item', () => {
    const sub = new StreamGroupSubscription<TestData>(joinMessage)
    const syncData = [
      { id: '1', name: 'A' },
      { id: '2', name: 'B' },
    ]

    sub.listener(makeMessage('sync', syncData))
    sub.listener(makeMessage('update', { id: '1', name: 'A1' }))
    sub.listener(makeMessage('update', { id: '1', name: 'A2' }, Date.now() - 1000))

    expect(sub.getState()).toEqual([
      { id: '1', name: 'A1' },
      { id: '2', name: 'B' },
    ])
  })
})
