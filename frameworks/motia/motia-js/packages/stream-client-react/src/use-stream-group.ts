import type { StreamSubscription } from '@motiadev/stream-client-browser'
import { useEffect, useRef, useState } from 'react'
import { useMotiaStream } from './use-motia-stream'

export type StreamGroupArgs<TData extends { id: string }> = {
  streamName: string
  groupId: string
  sortKey?: keyof TData
  setData?: (data: TData[]) => void
}

/**
 * A hook to get a group of items from a stream.
 *
 * @example
 * ```tsx
 * const { data } = useStreamGroup<{ id:string; name: string }>({
 *   streamName: 'my-stream',
 *   groupId: '123',
 * })
 *
 * return (
 *   <div>
 *     {data.map((item) => (
 *       <div key={item.id}>{item.name}</div>
 *     ))}
 *   </div>
 * )
 * ```
 */
export const useStreamGroup = <TData extends { id: string }>(args?: StreamGroupArgs<TData>) => {
  const { stream } = useMotiaStream()
  const [data, setData] = useState<TData[]>([])
  const subscriptionRef = useRef<StreamSubscription | null>(null)

  const { streamName, groupId, sortKey, setData: setDataCallback } = args || {}

  useEffect(() => {
    if (!streamName || !groupId || !stream) {
      console.error('useStreamGroup: streamName, groupId or stream is not defined', { streamName, groupId, stream })
      return
    }

    subscriptionRef.current = stream.subscribeGroup(streamName, groupId, sortKey)

    subscriptionRef.current.addChangeListener((data) => {
      const typedData = data as TData[]
      setData(typedData)
      setDataCallback?.(typedData)
    })

    return () => {
      subscriptionRef.current?.close()
      subscriptionRef.current = null
      setData([])
    }
  }, [stream, streamName, groupId, sortKey, setDataCallback])

  return { data, event: subscriptionRef.current }
}
