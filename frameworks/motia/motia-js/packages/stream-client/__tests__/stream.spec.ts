import type { SocketAdapter } from '../src/socket-adapter'
import { Stream } from '../src/stream'
import type { GroupEventMessage } from '../src/stream.types'

type TestData = { id: string; name: string; value: number }

class MockSocket implements SocketAdapter {
  listeners: Record<string, Function[]> = {}

  addEventListener(event: string, cb: Function) {
    if (!this.listeners[event]) this.listeners[event] = []
    this.listeners[event].push(cb)
  }

  connect(): void {}

  isOpen(): boolean {
    return true
  }

  onMessage(callback: (message: string) => void): void {
    this.addEventListener('message', callback)
  }

  onOpen(callback: () => void): void {
    this.addEventListener('open', callback)
  }

  onClose(callback: () => void): void {
    this.addEventListener('close', callback)
  }

  send(data: string) {
    this.listeners.message?.forEach((cb) => cb(data))
  }

  close() {
    this.listeners = {}
  }
}

function getSocket(stream: Stream): MockSocket {
  return (stream as any).ws as MockSocket
}

function makeGroupMessage(
  type: GroupEventMessage<TestData>['event']['type'],
  data: any,
  timestamp = Date.now(),
): GroupEventMessage<TestData> {
  return {
    streamName: 'test-stream',
    groupId: 'test-group',
    timestamp,
    event: { type, ...(type === 'event' ? { event: data } : { data }) },
  } as GroupEventMessage<TestData>
}

function makeItemMessage(
  type: GroupEventMessage<TestData>['event']['type'],
  data: any,
  timestamp = Date.now(),
): GroupEventMessage<TestData> {
  return {
    streamName: 'test-stream',
    groupId: 'test-group',
    id: '1',
    timestamp,
    event: { type, ...(type === 'event' ? { event: data } : { data }) },
  } as GroupEventMessage<TestData>
}

describe('Stream', () => {
  beforeEach(() => {
    global.WebSocket = MockSocket as never
  })

  afterEach(() => {
    delete (global as any).WebSocket
  })

  it('should sync group events', () => {
    const stream = new Stream(() => new MockSocket())
    const socket = getSocket(stream)

    const sub = stream.subscribeGroup('test-stream', 'test-group')

    const syncData = [
      { id: '1', name: 'A' },
      { id: '2', name: 'B' },
    ]

    socket.send(JSON.stringify(makeGroupMessage('sync', syncData)))
    expect(sub.getState()).toEqual(syncData)
  })

  it('should not sync item events in group subscriptions', () => {
    const stream = new Stream(() => new MockSocket())
    const socket = getSocket(stream)

    const sub = stream.subscribeGroup('test-stream', 'test-group')

    const syncData = [
      { id: '1', name: 'A' },
      { id: '2', name: 'B' },
    ]

    socket.send(JSON.stringify(makeGroupMessage('sync', syncData)))
    socket.send(JSON.stringify(makeItemMessage('sync', syncData[0])))

    expect(sub.getState()).toEqual(syncData)
  })
})
