import type { StreamSubscription } from '@motiadev/stream-client-browser'
import { type DependencyList, useEffect } from 'react'

type UseStreamEventHandler = {
  event: StreamSubscription | null
  type: string
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  listener: (event: any) => void
}

/**
 * A hook to handle custom stream events.
 *
 * @example
 * ```tsx
 * const { event } = useStreamItem({ streamName: 'my-stream', id: '123' })
 *
 * const onEventHandled = (event: any) => {
 *   // this is going to be called whenever 'on-custom-event' is sent from the server
 *   console.log(event)
 * }
 *
 * useStreamEventHandler({ event, type: 'on-custom-event', listener: onEventHandled }, [])
 * ```
 */
export const useStreamEventHandler = (
  { event, type, listener }: UseStreamEventHandler,
  dependencies: DependencyList,
) => {
  useEffect(() => {
    if (event) {
      event.onEvent(type, listener)
      return () => event.offEvent(type, listener)
    }
  }, [event, type, ...dependencies, listener])
}
