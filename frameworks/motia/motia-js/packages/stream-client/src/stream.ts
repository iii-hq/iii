import { v4 as uuidv4 } from 'uuid'
import type { SocketAdapterFactory } from './adapter-factory'
import type { SocketAdapter } from './socket-adapter'
import type { BaseMessage, ItemEventMessage } from './stream.types'
import { StreamGroupSubscription } from './stream-group'
import { StreamItemSubscription } from './stream-item'
import type { StreamSubscription } from './stream-subscription'

export class Stream {
  private ws: SocketAdapter
  private listeners: { [channelId: string]: Set<StreamSubscription> } = {}

  constructor(private adapterFactory: SocketAdapterFactory) {
    this.ws = this.createSocket()
  }

  createSocket(): SocketAdapter {
    this.ws = this.adapterFactory()
    this.ws.onMessage((message) => this.messageListener(message))
    this.ws.onOpen(() => this.onSocketOpen())
    this.ws.onClose(() => this.onSocketClose())

    return this.ws
  }

  /**
   * Subscribe to an item in a stream.
   *
   * @argument streamName - The name of the stream to subscribe to.
   * @argument groupId - The id of the group to subscribe to.
   * @argument id - The id of the item to subscribe to.
   */
  subscribeItem<TData extends { id: string }>(
    streamName: string,
    groupId: string,
    id: string,
  ): StreamItemSubscription<TData> {
    const subscriptionId = uuidv4()
    const sub = { streamName, groupId, id, subscriptionId }
    const subscription = new StreamItemSubscription<TData>(sub)

    this.subscribe(subscription)

    return subscription
  }

  /**
   * Subscribe to a group in a stream.
   *
   * @argument streamName - The name of the stream to subscribe to.
   * @argument groupId - The id of the group to subscribe to.
   */
  subscribeGroup<TData extends { id: string }>(
    streamName: string,
    groupId: string,
    sortKey?: keyof TData,
  ): StreamGroupSubscription<TData> {
    const subscriptionId = uuidv4()
    const sub = { streamName, groupId, subscriptionId }
    const subscription = new StreamGroupSubscription<TData>(sub, sortKey)

    this.subscribe(subscription)

    return subscription
  }

  close() {
    this.listeners = {} // clean up all listeners
    this.ws.close()
  }

  private onSocketClose(): void {
    // retry to connect
    setTimeout(() => this.createSocket(), 2000)
  }

  private onSocketOpen(): void {
    Object.values(this.listeners).forEach((listeners) => {
      listeners.forEach((subscription) => this.join(subscription))
    })
  }

  messageListener(event: string): void {
    const message: ItemEventMessage<never> = JSON.parse(event)
    const room = this.roomName(message)

    this.listeners[room]?.forEach((listener) => listener.listener(message))

    // we need to discard sync to group subs when it's an item event
    if (message.id && message.event.type !== 'sync') {
      const groupRoom = this.roomName({
        streamName: message.streamName,
        groupId: message.groupId,
      })

      this.listeners[groupRoom]?.forEach((listener) => listener.listener(message))
    }
  }

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  private subscribe(subscription: StreamSubscription<any, any>): void {
    const room = this.roomName(subscription.sub)

    if (!this.listeners[room]) {
      this.listeners[room] = new Set()
    }

    this.listeners[room].add(subscription)
    this.join(subscription)

    subscription.onClose(() => {
      this.listeners[room]?.delete(subscription)
      this.leave(subscription)
    })
  }

  private join(subscription: StreamSubscription): void {
    if (this.ws.isOpen()) {
      this.ws.send(JSON.stringify({ type: 'join', data: subscription.sub }))
    }
  }

  private leave(subscription: StreamSubscription): void {
    if (this.ws.isOpen()) {
      this.ws.send(JSON.stringify({ type: 'leave', data: subscription.sub }))
    }
  }

  private roomName(message: Omit<BaseMessage, 'timestamp'>): string {
    return message.id
      ? `${message.streamName}:group:${message.groupId}:item:${message.id}`
      : `${message.streamName}:group:${message.groupId}`
  }
}
