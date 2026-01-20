import { useCallback, useEffect, useRef, useState } from 'react'
import { useWebSocketStore } from '../stores/websocket-store'

interface SubscriptionEntry {
  streamName: string
  groupId: string
  subscriptionId: string
  id?: string
}

const HEARTBEAT_INTERVAL = 30_000

export function useMotiaWebSocket() {
  const wsRef = useRef<WebSocket | null>(null)
  const reconnectTimeoutRef = useRef<number | null>(null)
  const countdownIntervalRef = useRef<number | null>(null)
  const heartbeatIntervalRef = useRef<number | null>(null)
  const reconnectAttemptsRef = useRef(0)
  const isUnmountedRef = useRef(false)
  const subscriptionsRef = useRef<Map<string, SubscriptionEntry>>(new Map())
  const [isConnected, setIsConnected] = useState(false)

  const {
    addConnection,
    updateConnection,
    removeConnection,
    addMessage,
    updateStats,
    setReconnectionState,
    setConnectionHealth,
  } = useWebSocketStore()

  const connectionIdRef = useRef<string>(`motia_ws_${Date.now()}`)

  const resolveWebSocketUrl = useCallback((): string => {
    const maybeOverride = (window as unknown as { __MOTIA_WS_URL?: string }).__MOTIA_WS_URL
    const url = new URL(maybeOverride || window.location.origin)
    if (!maybeOverride) {
      url.protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    }
    const authToken = (window as unknown as { __MOTIA_WS_AUTH_TOKEN?: string }).__MOTIA_WS_AUTH_TOKEN
    if (authToken) {
      url.searchParams.set('authToken', authToken)
    }
    return url.toString()
  }, [])

  const startHeartbeat = useCallback(() => {
    if (heartbeatIntervalRef.current) clearInterval(heartbeatIntervalRef.current)

    heartbeatIntervalRef.current = window.setInterval(() => {
      if (wsRef.current?.readyState === WebSocket.OPEN) {
        const pingTime = Date.now()
        setConnectionHealth({ lastPingAt: pingTime })
        try {
          wsRef.current.send(JSON.stringify({ type: 'ping', timestamp: pingTime }))
        } catch {
          // Ignore
        }
      }
    }, HEARTBEAT_INTERVAL)
  }, [setConnectionHealth])

  const stopHeartbeat = useCallback(() => {
    if (heartbeatIntervalRef.current) {
      clearInterval(heartbeatIntervalRef.current)
      heartbeatIntervalRef.current = null
    }
  }, [])

  const resubscribeAll = useCallback(() => {
    if (wsRef.current?.readyState !== WebSocket.OPEN) return

    subscriptionsRef.current.forEach((sub) => {
      const message = {
        type: 'join',
        data: {
          streamName: sub.streamName,
          groupId: sub.groupId,
          ...(sub.id && { id: sub.id }),
          subscriptionId: sub.subscriptionId,
        },
      }
      try {
        wsRef.current?.send(JSON.stringify(message))
      } catch {
        // Ignore
      }
    })
  }, [])

  useEffect(() => {
    isUnmountedRef.current = false
    const connectionId = connectionIdRef.current

    const clearReconnect = () => {
      if (reconnectTimeoutRef.current) {
        clearTimeout(reconnectTimeoutRef.current)
        reconnectTimeoutRef.current = null
      }
      if (countdownIntervalRef.current) {
        clearInterval(countdownIntervalRef.current)
        countdownIntervalRef.current = null
      }
    }

    const scheduleReconnect = () => {
      if (isUnmountedRef.current) return
      const attempt = reconnectAttemptsRef.current
      const delay = Math.min(30_000, 1_000 * 2 ** attempt)
      const nextRetryAt = Date.now() + delay
      reconnectAttemptsRef.current = attempt + 1

      setReconnectionState({
        isReconnecting: true,
        retryCount: reconnectAttemptsRef.current,
        nextRetryAt,
        lastError: null,
      })

      countdownIntervalRef.current = window.setInterval(() => {
        const remaining = Math.max(0, nextRetryAt - Date.now())
        if (remaining <= 0) {
          clearInterval(countdownIntervalRef.current!)
          countdownIntervalRef.current = null
        }
        setReconnectionState({ nextRetryAt: remaining > 0 ? nextRetryAt : null })
      }, 1000)

      clearReconnect()
      reconnectTimeoutRef.current = window.setTimeout(() => {
        if (countdownIntervalRef.current) {
          clearInterval(countdownIntervalRef.current)
          countdownIntervalRef.current = null
        }
        connect()
      }, delay)
    }

    const connect = () => {
      if (isUnmountedRef.current) return
      clearReconnect()

      const wsUrl = resolveWebSocketUrl()
      const ws = new WebSocket(wsUrl)
      wsRef.current = ws

      const store = useWebSocketStore.getState()
      const existing = store.connections.find((c) => c.id === connectionId)
      if (existing) {
        updateConnection(connectionId, { status: 'connecting' })
      } else {
        addConnection({
          id: connectionId,
          url: wsUrl,
          status: 'connecting',
          createdAt: Date.now(),
          messageCount: 0,
        })
      }

      ws.onopen = () => {
        reconnectAttemptsRef.current = 0
        setIsConnected(true)
        updateConnection(connectionId, { status: 'connected' })
        updateStats({ activeConnections: 1 })
        setReconnectionState({
          isReconnecting: false,
          retryCount: 0,
          nextRetryAt: null,
          lastError: null,
        })

        addMessage({
          id: `msg_${Date.now()}_open`,
          connectionId,
          type: 'received',
          data: { type: 'system', message: 'WebSocket connection established' },
          timestamp: Date.now(),
        })

        startHeartbeat()
        resubscribeAll()
      }

      ws.onmessage = (event) => {
        let data: string | object
        try {
          data = JSON.parse(event.data) as object
        } catch {
          data = event.data as string
        }

        if (typeof data === 'object' && (data as Record<string, unknown>).type === 'pong') {
          const pongData = data as { timestamp?: number }
          if (pongData.timestamp) {
            const latency = Date.now() - pongData.timestamp
            setConnectionHealth({ latency, lastPongAt: Date.now() })
          }
          return
        }

        addMessage({
          id: `msg_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`,
          connectionId,
          type: 'received',
          data,
          timestamp: Date.now(),
        })

        const store = useWebSocketStore.getState()
        const conn = store.connections.find((c) => c.id === connectionId)
        if (conn) {
          updateConnection(connectionId, { messageCount: conn.messageCount + 1 })
        }
      }

      ws.onerror = () => {
        updateConnection(connectionId, {
          status: 'error',
          errorMessage: 'WebSocket error occurred',
        })
        setReconnectionState({ lastError: 'WebSocket error occurred' })

        addMessage({
          id: `msg_${Date.now()}_error`,
          connectionId,
          type: 'received',
          data: { type: 'error', message: 'WebSocket error occurred' },
          timestamp: Date.now(),
        })
      }

      ws.onclose = () => {
        setIsConnected(false)
        updateConnection(connectionId, { status: 'disconnected' })
        updateStats({ activeConnections: 0 })
        stopHeartbeat()

        addMessage({
          id: `msg_${Date.now()}_close`,
          connectionId,
          type: 'received',
          data: { type: 'system', message: 'WebSocket connection closed' },
          timestamp: Date.now(),
        })

        scheduleReconnect()
      }
    }

    connect()

    return () => {
      isUnmountedRef.current = true
      clearReconnect()
      stopHeartbeat()
      wsRef.current?.close()
      removeConnection(connectionId)
    }
  }, [
    addConnection,
    updateConnection,
    removeConnection,
    addMessage,
    updateStats,
    resolveWebSocketUrl,
    setReconnectionState,
    setConnectionHealth,
    startHeartbeat,
    stopHeartbeat,
    resubscribeAll,
  ])

  const sendMessage = useCallback(
    (data: string | object) => {
      if (wsRef.current && wsRef.current.readyState === WebSocket.OPEN) {
        const message = typeof data === 'string' ? data : JSON.stringify(data)
        wsRef.current.send(message)

        const connectionId = connectionIdRef.current
        addMessage({
          id: `msg_${Date.now()}_sent`,
          connectionId,
          type: 'sent',
          data,
          timestamp: Date.now(),
        })

        const store = useWebSocketStore.getState()
        const conn = store.connections.find((c) => c.id === connectionId)
        if (conn) {
          updateConnection(connectionId, { messageCount: conn.messageCount + 1 })
        }
      }
    },
    [addMessage, updateConnection],
  )

  const subscribeToStream = useCallback(
    (streamName: string, groupId: string, id?: string) => {
      const subscriptionId = `sub_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`

      subscriptionsRef.current.set(subscriptionId, {
        streamName,
        groupId,
        subscriptionId,
        id,
      })

      const message = {
        type: 'join',
        data: {
          streamName,
          groupId,
          ...(id && { id }),
          subscriptionId,
        },
      }
      sendMessage(message)
      return subscriptionId
    },
    [sendMessage],
  )

  const unsubscribeFromStream = useCallback(
    (streamName: string, groupId: string, subscriptionId: string, id?: string) => {
      subscriptionsRef.current.delete(subscriptionId)

      const message = {
        type: 'leave',
        data: {
          streamName,
          groupId,
          ...(id && { id }),
          subscriptionId,
        },
      }
      sendMessage(message)
    },
    [sendMessage],
  )

  return {
    isConnected,
    sendMessage,
    subscribeToStream,
    unsubscribeFromStream,
    connectionId: connectionIdRef.current,
  }
}

export function useWebSocketConnections() {
  const { connections, updateStats } = useWebSocketStore()

  const fetchConnections = useCallback(async () => {
    updateStats({
      totalConnections: connections.length,
      activeConnections: connections.filter((c) => c.status === 'connected').length,
    })
  }, [connections, updateStats])

  useEffect(() => {
    fetchConnections()
  }, [fetchConnections])

  return { connections, refetch: fetchConnections }
}

export function useWebSocketMessages(connectionId: string | null) {
  const { messages, searchQuery } = useWebSocketStore()

  const filteredMessages = messages.filter((m) => {
    if (connectionId && m.connectionId !== connectionId) return false
    if (!searchQuery) return true

    const dataStr = typeof m.data === 'string' ? m.data : JSON.stringify(m.data)
    return dataStr.toLowerCase().includes(searchQuery.toLowerCase())
  })

  return { messages: filteredMessages }
}
