import type { CustomEvent, JoinMessage, Listener } from './stream.types'

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type CustomEventListener = (event: any) => void

export abstract class StreamSubscription<TData = unknown, TEventData = unknown> {
  private customEventListeners: Map<string, CustomEventListener[]> = new Map()
  private closeListeners: Set<() => void> = new Set()

  private onChangeListeners: Set<Listener<TData>> = new Set()
  private state: TData

  readonly sub: JoinMessage

  constructor(sub: JoinMessage, state: TData) {
    this.sub = sub
    this.state = state
  }

  abstract listener(message: TEventData): void

  protected onEventReceived(event: CustomEvent) {
    const customEventListeners = this.customEventListeners.get(event.type)

    if (customEventListeners) {
      const eventData = event.data
      customEventListeners.forEach((listener) => listener(eventData))
    }
  }

  /**
   * Add a custom event listener. This listener will be called whenever the custom event is received.
   */
  onEvent(type: string, listener: CustomEventListener) {
    const listeners = this.customEventListeners.get(type) || []
    this.customEventListeners.set(type, [...listeners, listener])
  }

  /**
   * Remove a custom event listener.
   */
  offEvent(type: string, listener: CustomEventListener) {
    const listeners = this.customEventListeners.get(type) || []
    this.customEventListeners.set(
      type,
      listeners.filter((l) => l !== listener),
    )
  }

  onClose(listener: () => void) {
    this.closeListeners.add(listener)
  }

  close() {
    this.closeListeners.forEach((listener) => listener())
    this.closeListeners.clear()
  }

  /**
   * Add a change listener. This listener will be called whenever the state of the group changes.
   */
  addChangeListener(listener: Listener<TData>) {
    this.onChangeListeners.add(listener)
  }

  /**
   * Remove a change listener.
   */
  removeChangeListener(listener: Listener<TData>) {
    this.onChangeListeners.delete(listener)
  }

  /**
   * Get the current state of the group.
   */
  getState(): TData {
    return this.state
  }

  protected setState(state: TData) {
    this.state = state
    this.onChangeListeners.forEach((listener) => listener(state))
  }
}
