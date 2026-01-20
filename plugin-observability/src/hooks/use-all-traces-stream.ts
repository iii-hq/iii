import type { StreamSubscription } from '@motiadev/stream-client-browser'
import { useMotiaStream } from '@motiadev/stream-client-react'
import { useEffect, useRef } from 'react'
import { useObservabilityStore } from '../stores/use-observability-store'
import type { Trace } from '../types/observability'

const streamName = 'motia-trace'

export const useAllTracesStream = () => {
  const { stream } = useMotiaStream()
  const traceGroupMetas = useObservabilityStore((state) => state.traceGroupMetas)
  const setTracesForGroup = useObservabilityStore((state) => state.setTracesForGroup)
  const subscriptionsRef = useRef<Map<string, StreamSubscription>>(new Map())

  useEffect(() => {
    if (!stream) return

    const currentGroupIds = new Set(traceGroupMetas.map((meta) => meta.id))
    const subscribedGroupIds = new Set(subscriptionsRef.current.keys())

    traceGroupMetas.forEach((meta) => {
      if (!subscriptionsRef.current.has(meta.id)) {
        const subscription = stream.subscribeGroup(streamName, meta.id)

        subscription.addChangeListener((data) => {
          const traces = data as Trace[]
          setTracesForGroup(meta.id, traces)
        })

        subscriptionsRef.current.set(meta.id, subscription)
      }
    })

    subscribedGroupIds.forEach((groupId) => {
      if (!currentGroupIds.has(groupId)) {
        const subscription = subscriptionsRef.current.get(groupId)
        if (subscription) {
          subscription.close()
          subscriptionsRef.current.delete(groupId)
        }
      }
    })

    return () => {
      subscriptionsRef.current.forEach((subscription) => {
        subscription.close()
      })
      subscriptionsRef.current.clear()
    }
  }, [stream, traceGroupMetas, setTracesForGroup])
}
