import type { StreamSubscription } from '@motiadev/stream-client-browser'
import { useCallback, useEffect, useState } from 'react'
import { useMotiaStream } from './use-motia-stream'

export type StreamItemArgs = {
  streamName: string
  groupId: string
  id?: string
}

/**
 * A hook to get a single item from a stream.
 *
 * @example
 * ```tsx
 * const { data } = useStreamItem<{ id:string; name: string }>({
 *   streamName: 'my-stream',
 *   groupId: '123',
 *   id: '123',
 * })
 *
 * return (
 *   <div>{data?.name}</div>
 * )
 * ```
 */
export const useStreamItem = <TData>(args?: StreamItemArgs) => {
  const { stream } = useMotiaStream()
  const [data, setData] = useState<TData | null | undefined>()
  const [event, setEvent] = useState<StreamSubscription | null>(null)
  const { streamName, groupId, id } = args || {}

  const handleChange = useCallback((data: unknown) => {
    setData(data as TData)
  }, [])

  useEffect(() => {
    if (!streamName || !groupId || !id || !stream) return

    const subscription = stream.subscribeItem(streamName, groupId, id)

    subscription.addChangeListener(handleChange)
    setEvent(subscription)

    return () => {
      setData(undefined)
      setEvent(null)
      subscription.close()
    }
  }, [stream, streamName, groupId, id, handleChange])

  return { data, event }
}
