import type { ItemEventMessage, JoinMessage } from './stream.types'
import { StreamSubscription } from './stream-subscription'

export class StreamItemSubscription<TData extends { id: string }> extends StreamSubscription<
  TData | null,
  ItemEventMessage<TData>
> {
  private lastEventTimestamp = 0

  constructor(sub: JoinMessage) {
    super(sub, null)
  }

  listener(message: ItemEventMessage<TData>): void {
    if (message.timestamp <= this.lastEventTimestamp) {
      return
    }

    this.lastEventTimestamp = message.timestamp

    if (message.event.type === 'sync' || message.event.type === 'create' || message.event.type === 'update') {
      this.setState(message.event.data)
    } else if (message.event.type === 'delete') {
      this.setState(null)
    } else if (message.event.type === 'event') {
      this.onEventReceived(message.event.event)
    }
  }
}
