import type { GroupEventMessage, JoinMessage } from './stream.types'
import { StreamSubscription } from './stream-subscription'

export class StreamGroupSubscription<TData extends { id: string }> extends StreamSubscription<
  TData[],
  GroupEventMessage<TData>
> {
  private lastTimestamp: number = 0
  private lastTimestampMap: Map<string, number> = new Map()

  constructor(
    sub: JoinMessage,
    private sortKey?: keyof TData,
  ) {
    super(sub, [])
  }

  private sort(state: TData[]): TData[] {
    const sortKey = this.sortKey

    if (sortKey) {
      return state.sort((a: TData, b: TData) => {
        const aValue = a[sortKey]
        const bValue = b[sortKey]

        if (aValue && bValue) {
          return aValue.toString().localeCompare(bValue.toString())
        }

        return 0
      })
    }

    return state
  }

  protected setState(state: TData[]): void {
    super.setState(this.sort(state))
  }

  listener(message: GroupEventMessage<TData>): void {
    if (message.event.type === 'sync') {
      if (message.timestamp < this.lastTimestamp) {
        return
      }

      this.lastTimestampMap = new Map()
      this.lastTimestamp = message.timestamp
      this.setState(message.event.data)
    } else if (message.event.type === 'create') {
      const id = message.event.data.id
      const state = this.getState()

      if (!state.find((item) => item.id === id)) {
        this.setState([...state, message.event.data])
      }
    } else if (message.event.type === 'update') {
      const messageData = message.event.data
      const messageDataId = messageData.id
      const state = this.getState()
      const currentItemTimestamp = this.lastTimestampMap.get(messageDataId)

      if (currentItemTimestamp && currentItemTimestamp >= message.timestamp) {
        return
      }

      this.lastTimestamp = message.timestamp
      this.lastTimestampMap.set(messageDataId, message.timestamp)
      this.setState(state.map((item) => (item.id === messageDataId ? messageData : item)))
    } else if (message.event.type === 'delete') {
      const messageDataId = message.event.data.id
      const state = this.getState()

      this.lastTimestamp = message.timestamp
      this.lastTimestampMap.set(messageDataId, message.timestamp)
      this.setState(state.filter((item) => item.id !== messageDataId))
    } else if (message.event.type === 'event') {
      this.onEventReceived(message.event.event)
    }
  }
}
