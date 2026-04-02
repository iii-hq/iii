import type { QueryClient } from '@tanstack/react-query'
import { getStreamsWs } from './config'
import type { MetricsSnapshot, StreamMessage } from './types/shared'

const DEVTOOLS_STREAM = 'iii:devtools:state'
const DEVTOOLS_METRICS_GROUP = 'metrics'

export function createMetricsSubscription(queryClient: QueryClient) {
  let ws: WebSocket | null = null
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null
  let isActive = false
  const subscriptionId = `console-${Date.now()}-${Math.random().toString(36).slice(2)}`

  const streamsWs = getStreamsWs()

  function connect() {
    if (!isActive) return

    try {
      ws = new WebSocket(streamsWs)

      ws.onopen = () => {
        const joinMessage = {
          type: 'join',
          data: {
            subscriptionId,
            streamName: DEVTOOLS_STREAM,
            groupId: DEVTOOLS_METRICS_GROUP,
          },
        }
        ws?.send(JSON.stringify(joinMessage))
      }

      ws.onmessage = (event) => {
        try {
          const message: StreamMessage = JSON.parse(event.data)

          switch (message.event.type) {
            case 'sync':
              if (message.event.data && Array.isArray(message.event.data)) {
                const metrics = message.event.data as MetricsSnapshot[]
                queryClient.setQueryData(['metrics-history'], {
                  history: metrics,
                  count: metrics.length,
                })
                if (metrics.length > 0) {
                  queryClient.setQueryData(['metrics'], metrics[metrics.length - 1])
                }
              }
              break

            case 'create':
            case 'update':
              if (message.event.data) {
                const metrics = message.event.data as MetricsSnapshot
                queryClient.setQueryData(['metrics'], metrics)
                queryClient.setQueryData(
                  ['metrics-history'],
                  (old: { history: MetricsSnapshot[]; count: number } | undefined) => {
                    const history = old?.history || []
                    const newHistory = [...history, metrics].slice(-100)
                    return { history: newHistory, count: newHistory.length }
                  },
                )
              }
              break
          }
        } catch {
          // Ignore parse errors
        }
      }

      ws.onerror = () => {
        // Error handling - will reconnect on close
      }

      ws.onclose = () => {
        if (isActive) {
          reconnectTimer = setTimeout(connect, 3000)
        }
      }
    } catch {
      if (isActive) {
        reconnectTimer = setTimeout(connect, 3000)
      }
    }
  }

  return {
    connect: () => {
      isActive = true
      connect()
    },
    disconnect: () => {
      isActive = false
      if (reconnectTimer) {
        clearTimeout(reconnectTimer)
        reconnectTimer = null
      }
      if (ws && ws.readyState !== WebSocket.CLOSED) {
        const leaveMessage = {
          type: 'leave',
          data: {
            subscriptionId,
            streamName: DEVTOOLS_STREAM,
            groupId: DEVTOOLS_METRICS_GROUP,
          },
        }
        try {
          if (ws.readyState === WebSocket.OPEN) {
            ws.send(JSON.stringify(leaveMessage))
          }
          ws.close()
        } catch {
          // Ignore close errors
        }
        ws = null
      }
    },
  }
}

export function createStreamCapture(queryClient: QueryClient) {
  let ws: WebSocket | null = null
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null
  let isActive = false
  let currentSubscriptions: string[] = []

  const streamsWs = getStreamsWs()

  function connect() {
    if (!isActive) return

    try {
      ws = new WebSocket(streamsWs)

      ws.onopen = () => {
        currentSubscriptions.forEach((stream) => {
          ws?.send(JSON.stringify({ type: 'join', data: { streamName: stream } }))
        })
      }

      ws.onmessage = (event) => {
        try {
          const message: StreamMessage = JSON.parse(event.data)
          queryClient.setQueryData(['stream-messages'], (old: StreamMessage[] | undefined) => {
            const messages = old || []
            return [message, ...messages].slice(0, 1000)
          })
        } catch {
          // Ignore parse errors
        }
      }

      ws.onclose = () => {
        if (isActive) {
          reconnectTimer = setTimeout(connect, 3000)
        }
      }
    } catch {
      if (isActive) {
        reconnectTimer = setTimeout(connect, 3000)
      }
    }
  }

  return {
    connect: (subscriptions: string[] = []) => {
      isActive = true
      currentSubscriptions = subscriptions
      // Initialize empty message list
      queryClient.setQueryData(['stream-messages'], [])
      connect()
    },
    disconnect: () => {
      isActive = false
      if (reconnectTimer) {
        clearTimeout(reconnectTimer)
        reconnectTimer = null
      }
      if (ws) {
        try {
          ws.close()
        } catch {
          // Ignore
        }
        ws = null
      }
    },
    subscribe: (stream: string) => {
      if (!currentSubscriptions.includes(stream)) {
        currentSubscriptions.push(stream)
      }
      if (ws?.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({ type: 'join', data: { streamName: stream } }))
      }
    },
    unsubscribe: (stream: string) => {
      currentSubscriptions = currentSubscriptions.filter((s) => s !== stream)
      if (ws?.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({ type: 'leave', data: { streamName: stream } }))
      }
    },
  }
}
