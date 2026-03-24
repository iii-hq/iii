import { useQuery } from '@tanstack/react-query'
import { createFileRoute } from '@tanstack/react-router'
import {
  ArrowDownLeft,
  ArrowLeftRight,
  ArrowUpRight,
  Check,
  Copy,
  Download,
  Eye,
  EyeOff,
  Layers,
  Pause,
  Play,
  Plus,
  Radio,
  Search,
  Trash2,
  X,
  Zap,
} from 'lucide-react'
import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from 'react'
import type { StreamMessage } from '@/api'
import { getConnectionInfo, streamsQuery } from '@/api'
import { useConfig } from '@/api/config-provider'
import { Button, Input } from '@/components/ui/card'
import { EmptyState } from '@/components/ui/empty-state'
import { JsonViewer } from '@/components/ui/json-viewer'
import { Pagination } from '@/components/ui/pagination'

export const Route = createFileRoute('/streams')({
  component: StreamsPage,
  loader: ({ context: { queryClient } }) => {
    void queryClient.prefetchQuery(streamsQuery)
  },
})

interface WebSocketMessageEntry {
  id: string
  timestamp: number
  direction: 'inbound' | 'outbound' | 'system'
  streamName?: string
  groupId?: string
  eventType: string
  data: unknown
  size: number
}

const DIRECTION_CONFIG = {
  inbound: {
    icon: ArrowDownLeft,
    color: 'text-cyan-400',
    bg: 'bg-cyan-500/10',
    label: 'Inbound',
    border: 'border-l-cyan-400',
  },
  outbound: {
    icon: ArrowUpRight,
    color: 'text-orange-400',
    bg: 'bg-orange-500/10',
    label: 'Outbound',
    border: 'border-l-orange-400',
  },
  system: {
    icon: Radio,
    color: 'text-muted',
    bg: 'bg-dark-gray/50',
    label: 'System',
    border: 'border-l-muted',
  },
}

const EVENT_TYPE_INFO: Record<string, { label: string; description: string; color: string }> = {
  create: {
    label: 'Create',
    description: 'New item added to stream',
    color: 'text-success',
  },
  update: {
    label: 'Update',
    description: 'Existing item modified',
    color: 'text-yellow',
  },
  delete: {
    label: 'Delete',
    description: 'Item removed from stream',
    color: 'text-error',
  },
  sync: {
    label: 'Sync',
    description: 'Initial data sync on subscription',
    color: 'text-purple-400',
  },
  subscribe: {
    label: 'Subscribe',
    description: 'Subscribed to stream updates',
    color: 'text-cyan-400',
  },
  unsubscribe: {
    label: 'Unsubscribe',
    description: 'Unsubscribed from stream',
    color: 'text-muted',
  },
  connected: {
    label: 'Connected',
    description: 'WebSocket connection established',
    color: 'text-success',
  },
  disconnected: {
    label: 'Disconnected',
    description: 'WebSocket connection closed',
    color: 'text-error',
  },
  error: {
    label: 'Error',
    description: 'An error occurred',
    color: 'text-error',
  },
  message: {
    label: 'Message',
    description: 'Generic message',
    color: 'text-foreground',
  },
  sent: {
    label: 'Sent',
    description: 'Message sent to server',
    color: 'text-orange-400',
  },
}

interface MessagesState {
  messages: WebSocketMessageEntry[]
  stats: {
    totalMessages: number
    inbound: number
    outbound: number
    totalBytes: number
    latency: number | null
    lastPingTime: number | null
  }
}

type MessagesAction =
  | { type: 'add_message'; entry: WebSocketMessageEntry }
  | { type: 'clear'; latency: number | null; lastPingTime: number | null }
  | { type: 'set_latency'; latency: number }

function messagesReducer(state: MessagesState, action: MessagesAction): MessagesState {
  switch (action.type) {
    case 'add_message': {
      const { entry } = action
      return {
        messages: [entry, ...state.messages].slice(0, 1000),
        stats: {
          ...state.stats,
          totalMessages: state.stats.totalMessages + 1,
          inbound: entry.direction === 'inbound' ? state.stats.inbound + 1 : state.stats.inbound,
          outbound:
            entry.direction === 'outbound' ? state.stats.outbound + 1 : state.stats.outbound,
          totalBytes: state.stats.totalBytes + entry.size,
        },
      }
    }
    case 'clear':
      return {
        messages: [],
        stats: {
          totalMessages: 0,
          inbound: 0,
          outbound: 0,
          totalBytes: 0,
          latency: action.latency,
          lastPingTime: action.lastPingTime,
        },
      }
    case 'set_latency':
      return { ...state, stats: { ...state.stats, latency: action.latency } }
    default:
      return state
  }
}

const INITIAL_MESSAGES_STATE: MessagesState = {
  messages: [],
  stats: {
    totalMessages: 0,
    inbound: 0,
    outbound: 0,
    totalBytes: 0,
    latency: null,
    lastPingTime: null,
  },
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}

function formatTime(timestamp: number): string {
  const date = new Date(timestamp)
  return date.toLocaleTimeString('en-US', {
    hour12: false,
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    fractionalSecondDigits: 3,
  })
}

function StreamsPage() {
  const [searchQuery, setSearchQuery] = useState('')
  const [wsConnected, setWsConnected] = useState(false)
  const [showSystem, setShowSystem] = useState(false)
  const [isPaused, setIsPaused] = useState(false)

  const [{ messages, stats }, messagesDispatch] = useReducer(
    messagesReducer,
    INITIAL_MESSAGES_STATE,
  )
  const [selectedMessageId, setSelectedMessageId] = useState<string | null>(null)
  const [directionFilter, setDirectionFilter] = useState<'all' | 'inbound' | 'outbound'>('all')
  const [streamFilter, setStreamFilter] = useState<string | null>(null)
  const [copiedId, setCopiedId] = useState<string | null>(null)
  const [autoScroll] = useState(true)
  const [currentPage, setCurrentPage] = useState(1)
  const [pageSize, setPageSize] = useState(50)

  const wsRef = useRef<WebSocket | null>(null)
  const messagesContainerRef = useRef<HTMLDivElement>(null)
  const messageIdCounter = useRef(0)
  const subscriptionsRef = useRef<Map<string, { groupId: string; subscriptionId: string }>>(
    new Map(),
  ) // Map<streamName, {groupId, subscriptionId}>
  const lastPingTimeRef = useRef<number | null>(null)
  const isPausedRef = useRef(false)

  const [subscribedStreams, setSubscribedStreams] = useState<
    Array<{ streamName: string; groupId: string; subscriptionId: string }>
  >([])
  const [showSubscribeModal, setShowSubscribeModal] = useState(false)
  const [newStreamName, setNewStreamName] = useState('')
  const [selectedGroupId, setSelectedGroupId] = useState<string>('')

  const { data: streamsData } = useQuery(streamsQuery)
  const streams = streamsData?.streams || []
  const config = useConfig()
  const websocketPort = streamsData?.websocket_port || config.wsPort

  // Computed values for group selection
  const currentStream = streams.find((s) => s.id === newStreamName)
  const availableGroups = currentStream?.groups || []

  const { streamsWs } = getConnectionInfo()

  useEffect(() => {
    isPausedRef.current = isPaused
  }, [isPaused])

  useEffect(() => {
    let ws: WebSocket | null = null
    let reconnectTimer: ReturnType<typeof setTimeout> | null = null
    let mounted = true

    const addMessage = (msg: Omit<WebSocketMessageEntry, 'id' | 'timestamp' | 'size'>) => {
      const id = `msg_${Date.now()}_${messageIdCounter.current++}`
      const dataStr = typeof msg.data === 'string' ? msg.data : JSON.stringify(msg.data)
      const size = new Blob([dataStr]).size
      const entry: WebSocketMessageEntry = { id, timestamp: Date.now(), size, ...msg }
      messagesDispatch({ type: 'add_message', entry })
    }

    const connect = () => {
      if (!mounted) return

      try {
        ws = new WebSocket(streamsWs)
        wsRef.current = ws

        ws.onopen = () => {
          if (!mounted) return
          setWsConnected(true)

          addMessage({
            direction: 'system',
            eventType: 'connected',
            data: {
              message: 'WebSocket connection established',
              url: streamsWs,
            },
          })
        }

        ws.onmessage = (event) => {
          if (!mounted || isPausedRef.current) return

          try {
            const data = JSON.parse(event.data)

            if (data.type === 'pong' && lastPingTimeRef.current) {
              const latency = Date.now() - lastPingTimeRef.current
              messagesDispatch({ type: 'set_latency', latency })
              return
            }

            const message = data as StreamMessage
            addMessage({
              direction: 'inbound',
              streamName: message.streamName,
              groupId: message.groupId,
              eventType: message.event?.type || 'message',
              data: message,
            })
          } catch {
            addMessage({
              direction: 'inbound',
              eventType: 'raw',
              data: event.data,
            })
          }
        }

        ws.onclose = () => {
          if (!mounted) return
          setWsConnected(false)

          addMessage({
            direction: 'system',
            eventType: 'disconnected',
            data: { message: 'WebSocket connection closed' },
          })

          reconnectTimer = setTimeout(connect, 3000)
        }

        ws.onerror = () => {
          addMessage({
            direction: 'system',
            eventType: 'error',
            data: { message: 'WebSocket error occurred' },
          })
        }
      } catch {
        reconnectTimer = setTimeout(connect, 3000)
      }
    }

    connect()

    return () => {
      mounted = false
      if (reconnectTimer) clearTimeout(reconnectTimer)
      if (ws) ws.close()
    }
  }, [streamsWs])

  useEffect(() => {
    if (autoScroll && messagesContainerRef.current) {
      messagesContainerRef.current.scrollTop = 0
    }
  }, [autoScroll])

  const subscribeToStream = useCallback((streamName: string, groupId: string = 'all') => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      const subscriptionId = `console-${Date.now()}-${Math.random().toString(36).slice(2)}`
      const message = {
        type: 'join',
        data: { streamName, groupId, subscriptionId },
      }
      wsRef.current.send(JSON.stringify(message))
      subscriptionsRef.current.set(`${streamName}:${groupId}`, { groupId, subscriptionId })
      setSubscribedStreams((prev) => {
        const existing = prev.find((s) => s.streamName === streamName && s.groupId === groupId)
        if (existing) return prev
        return [...prev, { streamName, groupId, subscriptionId }]
      })

      const size = new Blob([JSON.stringify(message)]).size
      const entry: WebSocketMessageEntry = {
        id: `msg_${Date.now()}_${messageIdCounter.current++}`,
        timestamp: Date.now(),
        direction: 'outbound',
        streamName,
        groupId,
        eventType: 'subscribe',
        data: message,
        size,
      }
      messagesDispatch({ type: 'add_message', entry })

      return subscriptionId
    }
    return null
  }, [])

  const unsubscribeFromStream = useCallback((streamName: string, groupId: string) => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      const subscription = subscriptionsRef.current.get(`${streamName}:${groupId}`)
      if (!subscription) {
        console.warn(`No subscription found for stream: ${streamName}`)
        return
      }

      const message = {
        type: 'leave',
        data: {
          streamName,
          groupId,
          subscriptionId: subscription.subscriptionId,
        },
      }
      wsRef.current.send(JSON.stringify(message))
      subscriptionsRef.current.delete(`${streamName}:${groupId}`)
      setSubscribedStreams((prev) =>
        prev.filter((s) => !(s.streamName === streamName && s.groupId === groupId)),
      )
    }
  }, [])

  const filteredMessages = useMemo(() => {
    return messages.filter((msg) => {
      if (directionFilter !== 'all' && msg.direction !== directionFilter) return false
      if (streamFilter && msg.streamName !== streamFilter) return false
      if (!showSystem && msg.direction === 'system') return false

      if (searchQuery) {
        const dataStr = JSON.stringify(msg.data).toLowerCase()
        const searchLower = searchQuery.toLowerCase()
        return (
          dataStr.includes(searchLower) ||
          msg.streamName?.toLowerCase().includes(searchLower) ||
          msg.eventType.toLowerCase().includes(searchLower)
        )
      }

      return true
    })
  }, [messages, directionFilter, streamFilter, showSystem, searchQuery])

  const totalPages = Math.max(1, Math.ceil(filteredMessages.length / pageSize))
  const paginatedMessages = useMemo(() => {
    const start = (currentPage - 1) * pageSize
    return filteredMessages.slice(start, start + pageSize)
  }, [filteredMessages, currentPage, pageSize])

  const selectedMessage = messages.find((m) => m.id === selectedMessageId)

  const uniqueStreams = useMemo(() => {
    const streamSet = new Set<string>()
    messages.forEach((m) => {
      if (m.streamName) streamSet.add(m.streamName)
    })
    return Array.from(streamSet)
  }, [messages])

  const clearMessages = () => {
    messagesDispatch({ type: 'clear', latency: stats.latency, lastPingTime: stats.lastPingTime })
    setSelectedMessageId(null)
  }

  const copyToClipboard = (text: string, id: string) => {
    navigator.clipboard.writeText(text)
    setCopiedId(id)
    setTimeout(() => setCopiedId(null), 2000)
  }

  const exportMessages = () => {
    const data = filteredMessages.map((m) => ({
      timestamp: new Date(m.timestamp).toISOString(),
      direction: m.direction,
      streamName: m.streamName,
      groupId: m.groupId,
      eventType: m.eventType,
      size: m.size,
      data: m.data,
    }))
    const blob = new Blob([JSON.stringify(data, null, 2)], {
      type: 'application/json',
    })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = `ws-messages-${new Date().toISOString().slice(0, 19).replace(/:/g, '-')}.json`
    a.click()
    URL.revokeObjectURL(url)
  }

  return (
    <div className="flex flex-col h-full bg-background text-foreground">
      {/* Header */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 px-3 md:px-5 py-3 md:py-4 bg-dark-gray/30 border-b border-border">
        <div className="flex items-center gap-2 md:gap-4 flex-wrap">
          <h1 className="font-sans font-semibold text-lg tracking-tight flex items-center gap-2">
            <Layers className="w-5 h-5 text-green-400" />
            Streams
          </h1>
          <div className="font-sans text-sm text-secondary bg-dark-gray/50 px-1.5 md:px-2 py-0.5 md:py-1 rounded flex items-center gap-1 hidden sm:flex">
            <ArrowLeftRight className="w-3 h-3" />
            <span className="hidden md:inline">WebSocket Monitor</span>
            <span className="md:hidden">WS</span>
          </div>
        </div>

        <div className="flex items-center gap-1.5 md:gap-2">
          <Button
            variant={isPaused ? 'accent' : 'ghost'}
            size="sm"
            onClick={() => setIsPaused(!isPaused)}
            className="h-7 text-xs"
          >
            {isPaused ? <Play className="w-3 h-3 mr-1.5" /> : <Pause className="w-3 h-3 mr-1.5" />}
            {isPaused ? 'Resume' : 'Pause'}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={exportMessages}
            disabled={messages.length === 0}
            className="h-7 text-xs text-muted hover:text-foreground"
          >
            <Download className="w-3 h-3 mr-1.5" />
            Export
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={clearMessages}
            disabled={messages.length === 0}
            className="h-7 text-xs text-muted hover:text-foreground"
          >
            <Trash2 className="w-3 h-3 mr-1.5" />
            Clear
          </Button>
          <Button
            variant={showSystem ? 'accent' : 'ghost'}
            size="sm"
            onClick={() => setShowSystem(!showSystem)}
            className="h-7 text-xs"
          >
            {showSystem ? (
              <Eye className="w-3 h-3 mr-1.5" />
            ) : (
              <EyeOff className="w-3 h-3 mr-1.5" />
            )}
            <span className={showSystem ? '' : 'line-through opacity-60'}>System</span>
          </Button>
        </div>
      </div>

      {/* Stats Bar */}
      <div className="flex items-center gap-6 px-5 py-2 bg-dark-gray/20 border-b border-border text-xs">
        <div className="flex items-center gap-2">
          <span className="text-muted">Messages:</span>
          <span className="font-bold tabular-nums">{stats.totalMessages}</span>
        </div>
        <div className="flex items-center gap-2">
          <ArrowDownLeft className="w-3 h-3 text-cyan-400" />
          <span className="text-cyan-400 font-medium tabular-nums">{stats.inbound}</span>
          <span className="text-muted">inbound</span>
        </div>
        <div className="flex items-center gap-2">
          <ArrowUpRight className="w-3 h-3 text-orange-400" />
          <span className="text-orange-400 font-medium tabular-nums">{stats.outbound}</span>
          <span className="text-muted">outbound</span>
        </div>
        <div className="flex items-center gap-2">
          <span className="text-muted">Size:</span>
          <span className="font-mono tabular-nums">{formatBytes(stats.totalBytes)}</span>
        </div>
        <div className="flex-1" />
        <div className="text-muted font-mono">ws://localhost:{websocketPort}</div>
      </div>

      {/* Subscriptions Bar */}
      <div className="flex items-center gap-3 px-5 py-2 bg-dark-gray/20 border-b border-border">
        <span className="font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted">
          Subscriptions
        </span>

        <div className="flex items-center gap-2">
          {subscribedStreams.map((sub) => (
            <div
              key={`${sub.streamName}-${sub.groupId}`}
              className="flex items-center gap-1.5 px-2 py-1 rounded-md bg-green-500/10 border border-green-500/30 text-xs"
            >
              <span className="w-1.5 h-1.5 rounded-full bg-green-400 animate-pulse" />
              <span className="text-green-400 font-medium">
                {sub.streamName.replace('iii:', '').replace('iii.', '')}
              </span>
              <span className="text-green-400/50 text-[10px]">({sub.groupId})</span>
              <button
                type="button"
                onClick={() => unsubscribeFromStream(sub.streamName, sub.groupId)}
                className="p-0.5 rounded hover:bg-green-500/20 text-green-400/60 hover:text-green-400"
              >
                <X className="w-3 h-3" />
              </button>
            </div>
          ))}
        </div>

        <Button
          variant="outline"
          size="sm"
          onClick={() => setShowSubscribeModal(true)}
          disabled={!wsConnected}
          className="h-6 text-[10px] px-2 border-dashed gap-1"
        >
          <Plus className="w-3 h-3" />
          Subscribe
        </Button>
      </div>

      {/* Filter Bar */}
      <div className="flex items-center gap-3 px-5 py-2 bg-dark-gray/10 border-b border-border">
        <div className="relative flex-1 max-w-sm">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted" />
          <Input
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Search..."
            className="pl-9 pr-9 h-9"
          />
          {searchQuery && (
            <button
              type="button"
              onClick={() => setSearchQuery('')}
              className="absolute right-3 top-1/2 -translate-y-1/2 text-muted hover:text-foreground"
            >
              <X className="w-4 h-4" />
            </button>
          )}
        </div>

        <div className="flex items-center gap-1 border-l border-border pl-3">
          <Button
            variant={directionFilter === 'all' ? 'accent' : 'ghost'}
            size="sm"
            onClick={() => setDirectionFilter('all')}
            className="h-7 text-xs px-2"
          >
            All
          </Button>
          <Button
            variant={directionFilter === 'inbound' ? 'accent' : 'ghost'}
            size="sm"
            onClick={() => setDirectionFilter('inbound')}
            className="h-7 text-xs px-2 gap-1"
          >
            <ArrowDownLeft className="w-3 h-3" />
            Inbound
          </Button>
          <Button
            variant={directionFilter === 'outbound' ? 'accent' : 'ghost'}
            size="sm"
            onClick={() => setDirectionFilter('outbound')}
            className="h-7 text-xs px-2 gap-1"
          >
            <ArrowUpRight className="w-3 h-3" />
            Outbound
          </Button>
        </div>

        {uniqueStreams.length > 0 && (
          <div className="flex items-center gap-1 border-l border-border pl-3">
            <span className="text-xs text-muted mr-1">Stream:</span>
            <Button
              variant={streamFilter === null ? 'accent' : 'ghost'}
              size="sm"
              onClick={() => setStreamFilter(null)}
              className="h-6 text-[10px] px-2"
            >
              All
            </Button>
            {uniqueStreams.slice(0, 4).map((stream) => (
              <Button
                key={stream}
                variant={streamFilter === stream ? 'accent' : 'ghost'}
                size="sm"
                onClick={() => setStreamFilter(stream)}
                className="h-6 text-[10px] px-2 max-w-[100px] truncate"
              >
                {stream.replace('iii:', '').replace('iii.', '')}
              </Button>
            ))}
            {uniqueStreams.length > 4 && (
              <span className="text-[10px] text-muted">+{uniqueStreams.length - 4} more</span>
            )}
          </div>
        )}
      </div>

      {/* Main Content - Message List + Detail Panel */}
      <div className={`flex-1 flex overflow-hidden ${selectedMessage ? '' : ''}`}>
        {/* Message List */}
        <div
          className={`flex flex-col flex-1 overflow-hidden ${selectedMessage ? 'border-r border-border' : ''}`}
        >
          <div ref={messagesContainerRef} className="flex-1 overflow-auto">
            {filteredMessages.length === 0 ? (
              <EmptyState
                icon={Layers}
                title={
                  messages.length === 0 ? 'No active streams' : 'No messages match your filters'
                }
                description={
                  messages.length === 0
                    ? 'Streams appear when your functions publish messages'
                    : 'Try adjusting your search or filters'
                }
                className="h-64"
              />
            ) : (
              <div className="divide-y divide-border/30">
                {paginatedMessages.map((msg) => {
                  const config = DIRECTION_CONFIG[msg.direction]
                  const Icon = config.icon
                  const isSelected = selectedMessageId === msg.id

                  return (
                    <button
                      key={msg.id}
                      type="button"
                      className={`flex items-center font-mono cursor-pointer text-[12px] h-8 px-3 border-l-2 transition-colors w-full text-left
                        ${isSelected ? 'bg-primary/10 border-l-primary' : `hover:bg-dark-gray/30 ${config.border}`}
                      `}
                      onClick={() => setSelectedMessageId(isSelected ? null : msg.id)}
                    >
                      <div className="flex items-center gap-2 w-[80px] shrink-0">
                        <Icon className={`w-3 h-3 ${config.color}`} />
                        <span className="text-muted text-[11px]">{formatTime(msg.timestamp)}</span>
                      </div>

                      {msg.streamName && (
                        <div className="shrink-0 ml-2 px-1.5 py-0.5 rounded bg-dark-gray/50 text-[10px] text-muted max-w-[120px] truncate">
                          {msg.streamName.replace('iii:', '').replace('__motia.', '')}
                        </div>
                      )}

                      <div
                        className={`shrink-0 ml-2 px-1.5 py-0.5 rounded text-[10px] ${config.bg} ${EVENT_TYPE_INFO[msg.eventType]?.color || config.color}`}
                        title={EVENT_TYPE_INFO[msg.eventType]?.description || msg.eventType}
                      >
                        {msg.eventType}
                      </div>

                      <div className="flex-1 ml-3 truncate text-foreground/80">
                        {typeof msg.data === 'string'
                          ? msg.data.slice(0, 100)
                          : JSON.stringify(msg.data).slice(0, 100)}
                      </div>

                      <div className="shrink-0 ml-2 text-[10px] text-muted tabular-nums">
                        {formatBytes(msg.size)}
                      </div>
                    </button>
                  )
                })}
              </div>
            )}
          </div>

          {/* Pagination */}
          {filteredMessages.length > 0 && (
            <div className="flex-shrink-0 bg-background/95 backdrop-blur border-t border-border px-3 py-2">
              <Pagination
                currentPage={currentPage}
                totalPages={totalPages}
                totalItems={filteredMessages.length}
                pageSize={pageSize}
                onPageChange={setCurrentPage}
                onPageSizeChange={setPageSize}
                pageSizeOptions={[25, 50, 100, 250]}
              />
            </div>
          )}
        </div>

        {/* Detail Panel */}
        {selectedMessage && (
          <div className="w-[400px] flex flex-col overflow-hidden bg-dark-gray/10">
            <div className="p-4 border-b border-border sticky top-0 bg-dark-gray/80 backdrop-blur z-10">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  {(() => {
                    const config = DIRECTION_CONFIG[selectedMessage.direction]
                    const Icon = config.icon
                    return (
                      <>
                        <Icon className={`w-4 h-4 ${config.color}`} />
                        <span
                          className={`px-2 py-0.5 rounded text-[10px] font-semibold uppercase ${config.bg} ${config.color}`}
                        >
                          {config.label}
                        </span>
                      </>
                    )
                  })()}
                </div>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => setSelectedMessageId(null)}
                  className="h-6 w-6 p-0"
                >
                  <X className="w-3.5 h-3.5" />
                </Button>
              </div>
              <p className="text-xs text-muted mt-2 font-mono">
                {new Date(selectedMessage.timestamp).toISOString()}
              </p>
            </div>

            <div className="flex-1 overflow-auto p-4 space-y-4">
              <div className="grid grid-cols-2 gap-3">
                <div className="bg-dark-gray/30 rounded p-2">
                  <div className="font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted mb-1">
                    Event Type
                  </div>
                  <div
                    className={`text-xs font-medium ${EVENT_TYPE_INFO[selectedMessage.eventType]?.color || ''}`}
                  >
                    {selectedMessage.eventType}
                  </div>
                  {EVENT_TYPE_INFO[selectedMessage.eventType]?.description && (
                    <div className="text-[9px] text-muted mt-0.5">
                      {EVENT_TYPE_INFO[selectedMessage.eventType].description}
                    </div>
                  )}
                </div>
                <div className="bg-dark-gray/30 rounded p-2">
                  <div className="font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted mb-1">
                    Size
                  </div>
                  <div className="text-xs font-mono">{formatBytes(selectedMessage.size)}</div>
                </div>
              </div>

              {selectedMessage.streamName && (
                <div>
                  <div className="font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted mb-2">
                    Stream
                  </div>
                  <code className="font-mono text-[13px] bg-dark-gray px-2 py-1.5 rounded block">
                    {selectedMessage.streamName}
                  </code>
                </div>
              )}

              {selectedMessage.groupId && (
                <div>
                  <div className="font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted mb-2">
                    Group ID
                  </div>
                  <code className="font-mono text-[13px] bg-dark-gray px-2 py-1.5 rounded block">
                    {selectedMessage.groupId}
                  </code>
                </div>
              )}

              <div>
                <div className="font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted mb-2 flex items-center gap-2">
                  Data
                  <button
                    type="button"
                    onClick={() =>
                      copyToClipboard(JSON.stringify(selectedMessage.data, null, 2), 'data')
                    }
                    className="p-0.5 rounded hover:bg-dark-gray/50 transition-colors"
                  >
                    {copiedId === 'data' ? (
                      <Check className="w-3 h-3 text-success" />
                    ) : (
                      <Copy className="w-3 h-3" />
                    )}
                  </button>
                </div>
                <div className="bg-dark-gray px-3 py-2 rounded overflow-x-auto max-h-[400px] overflow-y-auto">
                  <JsonViewer data={selectedMessage.data} collapsed={false} maxDepth={6} />
                </div>
              </div>
            </div>
          </div>
        )}
      </div>

      {/* Subscribe Modal */}
      {showSubscribeModal && (
        <div className="fixed inset-0 bg-background/80 backdrop-blur-sm flex items-center justify-center z-50">
          <div className="bg-background border border-border rounded-lg shadow-xl w-full max-w-md">
            <div className="flex items-center justify-between px-4 py-3 border-b border-border">
              <h3 className="font-sans font-semibold flex items-center gap-2">
                <Layers className="w-4 h-4 text-green-400" />
                Subscribe to Stream
              </h3>
              <button
                type="button"
                onClick={() => {
                  setShowSubscribeModal(false)
                  setNewStreamName('')
                  setSelectedGroupId('')
                }}
                className="p-1 rounded hover:bg-dark-gray"
              >
                <X className="w-4 h-4" />
              </button>
            </div>

            <div className="p-4 space-y-4">
              <div>
                <label htmlFor="stream-name" className="text-xs text-muted block mb-1.5">
                  Stream Name
                </label>
                <Input
                  id="stream-name"
                  value={newStreamName}
                  onChange={(e) => setNewStreamName(e.target.value)}
                  placeholder="e.g., todo, iii.logs, my-stream"
                  className="font-mono"
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' && newStreamName && selectedGroupId) {
                      subscribeToStream(newStreamName, selectedGroupId)
                      setShowSubscribeModal(false)
                      setNewStreamName('')
                      setSelectedGroupId('')
                    }
                  }}
                />
              </div>

              <div>
                <span className="text-xs text-muted block mb-1.5">Group</span>
                <div className="flex flex-wrap gap-1.5">
                  {/* Show only groups returned from API */}
                  {availableGroups.length > 0 ? (
                    availableGroups.map((group) => (
                      <button
                        key={group}
                        type="button"
                        onClick={() => setSelectedGroupId(group)}
                        className={`px-2.5 py-1.5 rounded text-xs font-mono transition-colors border ${
                          selectedGroupId === group
                            ? 'bg-green-500/20 border-green-500/50 text-green-400'
                            : 'bg-dark-gray/30 border-border text-muted hover:bg-dark-gray hover:text-foreground'
                        }`}
                      >
                        {group}
                      </button>
                    ))
                  ) : (
                    <p className="text-xs text-muted py-1.5">
                      {newStreamName
                        ? 'No groups available for this stream'
                        : 'Select a stream to see available groups'}
                    </p>
                  )}
                </div>

                {/* Helper text */}
                {availableGroups.length > 0 && (
                  <p className="text-[10px] text-muted mt-1.5">
                    Select a group from {newStreamName}
                  </p>
                )}
              </div>

              {streams.length > 0 && (
                <div>
                  <span className="font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted block mb-2">
                    Available Streams
                  </span>
                  <div className="flex flex-wrap gap-2 max-h-32 overflow-auto">
                    {streams
                      .filter((s) => !subscribedStreams.some((sub) => sub.streamName === s.id))
                      .map((stream) => (
                        <div key={stream.id} className="flex flex-col gap-1">
                          <button
                            type="button"
                            onClick={() => {
                              setNewStreamName(stream.id)
                              setSelectedGroupId(stream.groups[0] || '')
                            }}
                            className={`px-2 py-1 rounded text-xs font-mono transition-colors ${
                              stream.internal
                                ? 'bg-dark-gray/50 text-muted hover:bg-dark-gray hover:text-foreground'
                                : 'bg-green-500/10 text-green-400 hover:bg-green-500/20'
                            }`}
                          >
                            {stream.id}
                          </button>
                          {stream.groups.length > 0 && (
                            <div className="flex flex-wrap gap-1 pl-2">
                              {stream.groups.map((group) => (
                                <button
                                  key={group}
                                  type="button"
                                  onClick={() => {
                                    subscribeToStream(stream.id, group)
                                    setShowSubscribeModal(false)
                                    setNewStreamName('')
                                    setSelectedGroupId('')
                                  }}
                                  className="px-1.5 py-0.5 rounded text-[10px] font-mono bg-dark-gray/50 text-muted hover:bg-dark-gray hover:text-foreground"
                                  title={`Subscribe to ${stream.id} - ${group}`}
                                >
                                  {group}
                                </button>
                              ))}
                            </div>
                          )}
                        </div>
                      ))}
                  </div>
                </div>
              )}
            </div>

            <div className="flex items-center justify-end gap-2 px-4 py-3 border-t border-border">
              <Button
                variant="ghost"
                size="sm"
                onClick={() => {
                  setShowSubscribeModal(false)
                  setNewStreamName('')
                  setSelectedGroupId('')
                }}
              >
                Cancel
              </Button>
              <Button
                variant="accent"
                size="sm"
                onClick={() => {
                  if (newStreamName && selectedGroupId) {
                    subscribeToStream(newStreamName, selectedGroupId)
                    setShowSubscribeModal(false)
                    setNewStreamName('')
                    setSelectedGroupId('')
                  }
                }}
                disabled={!newStreamName || !selectedGroupId}
                className="gap-1.5"
              >
                <Zap className="w-3 h-3" />
                Subscribe
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
