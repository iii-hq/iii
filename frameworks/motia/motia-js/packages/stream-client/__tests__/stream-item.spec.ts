import type { ItemEventMessage, JoinMessage } from '../src/stream.types'
import { StreamItemSubscription } from '../src/stream-item'

type TestData = { id: string; name: string; value: number }

describe('StreamItemSubscription', () => {
  const joinMessage: JoinMessage = {
    streamName: 'test-stream',
    groupId: 'test-group',
    subscriptionId: 'sub-1',
  }

  function makeMessage(
    type: ItemEventMessage<TestData>['event']['type'],
    data: unknown,
    timestamp = Date.now(),
  ): ItemEventMessage<TestData> {
    return {
      streamName: 'test-stream',
      groupId: 'test-group',
      timestamp,
      event: { type, ...(type === 'event' ? { event: data } : { data }) },
    } as ItemEventMessage<TestData>
  }

  it('notifies change listeners on state change', () => {
    const sub = new StreamItemSubscription<TestData>(joinMessage)
    const listener = jest.fn()

    sub.addChangeListener(listener)
    const syncData: TestData = { id: '1', name: 'A', value: 1 }
    sub.listener(makeMessage('sync', syncData))
    expect(listener).toHaveBeenCalledWith(syncData)
  })

  it('should discard old events', () => {
    const sub = new StreamItemSubscription<TestData>(joinMessage)
    const syncData: TestData = { id: '1', name: 'A', value: 1 }
    const oldSyncData: TestData = { id: '1', name: 'B', value: 3 }

    sub.listener(makeMessage('sync', syncData))
    sub.listener(makeMessage('sync', oldSyncData, Date.now() - 1000))

    expect(sub.getState()).toEqual(syncData)
  })

  it('should update item', () => {
    const sub = new StreamItemSubscription<TestData>(joinMessage)
    const syncData: TestData = { id: '1', name: 'A', value: 1 }

    sub.listener(makeMessage('sync', syncData))
    sub.listener(makeMessage('update', { id: '1', name: 'A1', value: 2 }, Date.now() + 1000))

    expect(sub.getState()).toEqual({ id: '1', name: 'A1', value: 2 })
  })

  it('should remove item', () => {
    const sub = new StreamItemSubscription<TestData>(joinMessage)
    const syncData: TestData = { id: '1', name: 'A', value: 1 }

    sub.listener(makeMessage('sync', syncData))
    sub.listener(makeMessage('delete', { id: '1' }, Date.now() + 1000))

    expect(sub.getState()).toEqual(null)
  })
})
