import { Badge, Button, cn, Empty, EmptyDescription, EmptyTitle, Input, LevelDot } from '@motiadev/ui'
import { Activity, Check, Download, Plus, RefreshCw, Search, Trash2, Wifi, WifiOff, X } from 'lucide-react'
import type React from 'react'
import { useEffect, useMemo, useState } from 'react'
import { useMotiaWebSocket } from '../hooks/use-websocket-connections'
import { useWebSocketStore } from '../stores/websocket-store'
import { MessageViewer } from './message-viewer'

// Stream color palette - each stream gets a unique color
const STREAM_COLORS = [
  {
    bg: 'bg-emerald-500/15',
    border: 'border-emerald-500/40',
    text: 'text-emerald-400',
    dot: 'bg-emerald-400',
  },
  {
    bg: 'bg-blue-500/15',
    border: 'border-blue-500/40',
    text: 'text-blue-400',
    dot: 'bg-blue-400',
  },
  {
    bg: 'bg-amber-500/15',
    border: 'border-amber-500/40',
    text: 'text-amber-400',
    dot: 'bg-amber-400',
  },
  {
    bg: 'bg-rose-500/15',
    border: 'border-rose-500/40',
    text: 'text-rose-400',
    dot: 'bg-rose-400',
  },
  {
    bg: 'bg-violet-500/15',
    border: 'border-violet-500/40',
    text: 'text-violet-400',
    dot: 'bg-violet-400',
  },
  {
    bg: 'bg-cyan-500/15',
    border: 'border-cyan-500/40',
    text: 'text-cyan-400',
    dot: 'bg-cyan-400',
  },
]

// Preset streams for quick access
const PRESET_STREAMS = [
  { name: '__motia.logs', label: 'Logs' },
  { name: '__motia.api-endpoints', label: 'API' },
]

interface SubscribedStream {
  name: string
  label: string
  colorIndex: number
  messageCount: number
  lastActivity: number
  subscriptionId: string
}

export const WebSocketsPage: React.FC = () => {
  const { isConnected, sendMessage, subscribeToStream, unsubscribeFromStream, connectionId } = useMotiaWebSocket()
  const {
    selectedConnectionId,
    selectConnection,
    stats,
    clearMessages,
    messages,
    connections,
    searchQuery,
    setSearchQuery,
    reconnectionState,
    connectionHealth,
    exportMessages,
  } = useWebSocketStore()

  // Stream management
  const [subscribedStreams, setSubscribedStreams] = useState<SubscribedStream[]>([])
  const [activeFilter, setActiveFilter] = useState<string | null>(null)
  const [showAddStream, setShowAddStream] = useState(false)
  const [newStreamName, setNewStreamName] = useState('')
  const [groupId] = useState('default')
  const [colorCounter, setColorCounter] = useState(0)
  const [showSearch, setShowSearch] = useState(false)
  const [reconnectCountdown, setReconnectCountdown] = useState<number | null>(null)

  // Update reconnect countdown
  useEffect(() => {
    if (reconnectionState.nextRetryAt) {
      const updateCountdown = () => {
        const remaining = Math.max(0, Math.ceil((reconnectionState.nextRetryAt! - Date.now()) / 1000))
        setReconnectCountdown(remaining)
      }
      updateCountdown()
      const interval = setInterval(updateCountdown, 1000)
      return () => clearInterval(interval)
    } else {
      setReconnectCountdown(null)
    }
  }, [reconnectionState.nextRetryAt])

  // Auto-select the live connection if none selected
  useEffect(() => {
    if (!selectedConnectionId && connectionId && connections.length > 0) {
      selectConnection(connectionId)
    }
  }, [selectedConnectionId, connectionId, connections, selectConnection])

  // Auto-subscribe to both API and Logs streams when connected
  useEffect(() => {
    if (isConnected && subscribedStreams.length === 0) {
      PRESET_STREAMS.forEach((preset, index) => {
        const subscriptionId = subscribeToStream(preset.name, groupId)

        const newStream: SubscribedStream = {
          name: preset.name,
          label: preset.label,
          colorIndex: index % STREAM_COLORS.length,
          messageCount: 0,
          lastActivity: Date.now(),
          subscriptionId,
        }

        setSubscribedStreams((prev) => [...prev, newStream])
      })
      setColorCounter(PRESET_STREAMS.length)
    }
  }, [groupId, isConnected, subscribeToStream, subscribedStreams.length])

  // Count messages per stream
  const streamMessageCounts = useMemo(() => {
    const counts: Record<string, number> = {}
    messages.forEach((msg) => {
      if (typeof msg.data === 'object' && msg.data !== null) {
        const streamName = (msg.data as Record<string, unknown>).streamName as string
        if (streamName) {
          counts[streamName] = (counts[streamName] || 0) + 1
        }
      }
    })
    return counts
  }, [messages])

  const handleAddStream = (streamName: string, label?: string) => {
    if (!streamName || subscribedStreams.some((s) => s.name === streamName)) return

    const subscriptionId = subscribeToStream(streamName, groupId)

    const newStream: SubscribedStream = {
      name: streamName,
      label: label || streamName.replace('__motia.', '').replace(/-/g, ' '),
      colorIndex: colorCounter % STREAM_COLORS.length,
      messageCount: 0,
      lastActivity: Date.now(),
      subscriptionId,
    }

    setSubscribedStreams((prev) => [...prev, newStream])
    setColorCounter((prev) => prev + 1)
    setNewStreamName('')
    setShowAddStream(false)
  }

  const handleRemoveStream = (streamName: string) => {
    const stream = subscribedStreams.find((s) => s.name === streamName)
    if (stream?.subscriptionId) {
      unsubscribeFromStream(streamName, groupId, stream.subscriptionId)
    }
    setSubscribedStreams((prev) => prev.filter((s) => s.name !== streamName))
    if (activeFilter === streamName) {
      setActiveFilter(null)
    }
  }

  const handleClearAll = () => {
    clearMessages()
  }

  const handleExport = () => {
    const data = exportMessages()
    const blob = new Blob([data], { type: 'application/json' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = `websocket-messages-${new Date().toISOString().slice(0, 19).replace(/:/g, '-')}.json`
    document.body.appendChild(a)
    a.click()
    document.body.removeChild(a)
    URL.revokeObjectURL(url)
  }

  return (
    <div className="flex flex-col h-full bg-background text-foreground" role="region" aria-label="WebSocket Monitor">
      {/* Main Header Bar */}
      <div className="flex items-center justify-between px-5 py-3 bg-muted/50 border-b border-border">
        <div className="flex items-center gap-4">
          <h1 className="text-base font-semibold">WebSockets</h1>

          {/* Connection Status Badge */}
          {reconnectionState.isReconnecting ? (
            <Badge
              variant="warning"
              className="gap-2"
              aria-live="polite"
              aria-label={`Reconnecting in ${reconnectCountdown} seconds`}
            >
              <RefreshCw className="w-3 h-3 animate-spin" />
              Reconnecting
              {reconnectCountdown !== null && ` in ${reconnectCountdown}s`}
            </Badge>
          ) : (
            <Badge
              variant={isConnected ? 'success' : 'error'}
              className="gap-2"
              aria-label={isConnected ? 'Connected' : 'Disconnected'}
            >
              {isConnected ? <Wifi className="w-3 h-3" /> : <WifiOff className="w-3 h-3" />}
              {isConnected ? 'Connected' : 'Disconnected'}
            </Badge>
          )}

          {/* Latency Indicator */}
          {isConnected && connectionHealth.latency !== null && (
            <div
              className="flex items-center gap-1.5 text-xs text-muted-foreground"
              title={`Latency ${connectionHealth.latency}ms`}
            >
              <Activity
                className={cn(
                  'w-3 h-3',
                  connectionHealth.latency < 100
                    ? 'text-emerald-400'
                    : connectionHealth.latency < 300
                      ? 'text-amber-400'
                      : 'text-red-400',
                )}
              />
              <span className="font-mono tabular-nums">{connectionHealth.latency}ms</span>
            </div>
          )}
        </div>

        <div className="flex items-center gap-2">
          {/* Search Toggle */}
          {showSearch ? (
            <div className="flex items-center gap-2">
              <div className="relative">
                <Search className="absolute left-2 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-muted-foreground" />
                <Input
                  type="text"
                  value={searchQuery}
                  onChange={(e) => setSearchQuery(e.target.value)}
                  placeholder="Search messages..."
                  className="w-48 h-7 pl-7 text-xs"
                  aria-label="Search messages"
                  autoFocus
                  onKeyDown={(e) => {
                    if (e.key === 'Escape') {
                      setShowSearch(false)
                      setSearchQuery('')
                    }
                  }}
                />
              </div>
              <Button
                variant="ghost"
                size="icon"
                onClick={() => {
                  setShowSearch(false)
                  setSearchQuery('')
                }}
                className="h-7 w-7"
                aria-label="Close search"
              >
                <X className="w-3.5 h-3.5" />
              </Button>
            </div>
          ) : (
            <Button
              variant="ghost"
              size="icon"
              onClick={() => setShowSearch(true)}
              className="h-7 w-7 text-muted-foreground hover:text-foreground"
              aria-label="Open search"
            >
              <Search className="w-3.5 h-3.5" />
            </Button>
          )}

          {/* Message Count */}
          <div className="text-xs text-muted-foreground px-2">
            <span className="text-foreground font-medium tabular-nums">{stats.totalMessages}</span> messages
          </div>

          {/* Export Button */}
          <Button
            variant="ghost"
            size="sm"
            onClick={handleExport}
            disabled={stats.totalMessages === 0}
            className="text-xs text-muted-foreground hover:text-foreground"
            aria-label="Export messages as JSON"
          >
            <Download className="w-3 h-3 mr-1.5" />
            Export
          </Button>

          {/* Clear Button */}
          <Button
            variant="ghost"
            size="sm"
            onClick={handleClearAll}
            disabled={stats.totalMessages === 0}
            className="text-xs text-muted-foreground hover:text-foreground"
            aria-label="Clear all messages"
          >
            <Trash2 className="w-3 h-3 mr-1.5" />
            Clear
          </Button>
        </div>
      </div>

      {/* Reconnection Error Banner */}
      {reconnectionState.lastError && (
        <div className="px-5 py-2 bg-red-500/10 border-b border-red-500/20 text-red-400 text-xs" role="alert">
          {reconnectionState.lastError} · Attempt {reconnectionState.retryCount}
        </div>
      )}

      {/* Stream Subscription Bar */}
      <div className="px-5 py-3 bg-muted/30 border-b border-border/50" role="toolbar" aria-label="Stream filters">
        <div className="flex items-center gap-6">
          {/* Subscribed Streams Section */}
          <div className="flex items-center gap-3">
            <span className="text-[11px] font-medium text-muted-foreground uppercase tracking-wide">Streams</span>

            <div className="flex items-center gap-2" role="tablist">
              {/* All Tab */}
              <Button
                variant={activeFilter === null ? 'default' : 'ghost'}
                size="sm"
                onClick={() => setActiveFilter(null)}
                className="h-7 text-xs"
                role="tab"
                aria-selected={activeFilter === null}
                aria-label={`All streams, ${stats.totalMessages} messages`}
              >
                All
                <span
                  className={cn(
                    'ml-2 px-1.5 py-0.5 rounded text-[10px] font-medium tabular-nums',
                    activeFilter === null ? 'bg-primary-foreground/20' : 'bg-muted',
                  )}
                >
                  {stats.totalMessages}
                </span>
              </Button>

              {/* Stream Tabs */}
              {subscribedStreams.map((stream) => {
                const color = STREAM_COLORS[stream.colorIndex]
                const count = streamMessageCounts[stream.name] || 0
                const isActive = activeFilter === stream.name

                return (
                  <div
                    key={stream.name}
                    className={cn(
                      'group inline-flex items-center gap-2 pl-3 pr-2 py-1.5 rounded-md text-xs font-medium',
                      'transition-all duration-150 cursor-pointer',
                      isActive
                        ? `${color.bg} ${color.text} ring-1 ${color.border}`
                        : 'text-muted-foreground hover:text-foreground hover:bg-accent/50',
                    )}
                    onClick={() => setActiveFilter(isActive ? null : stream.name)}
                    role="tab"
                    aria-selected={isActive}
                    aria-label={`${stream.label} stream, ${count} messages`}
                    tabIndex={0}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter' || e.key === ' ') {
                        e.preventDefault()
                        setActiveFilter(isActive ? null : stream.name)
                      }
                    }}
                  >
                    <LevelDot level={isActive ? 'success' : 'neutral'} />
                    <span className="capitalize">{stream.label}</span>
                    <span
                      className={cn(
                        'px-1.5 py-0.5 rounded text-[10px] font-medium tabular-nums',
                        isActive ? 'bg-white/10' : 'bg-muted text-muted-foreground',
                      )}
                    >
                      {count}
                    </span>
                    <button
                      onClick={(e) => {
                        e.stopPropagation()
                        handleRemoveStream(stream.name)
                      }}
                      className={cn(
                        'p-0.5 rounded transition-all',
                        isActive
                          ? 'opacity-60 hover:opacity-100 hover:bg-white/10'
                          : 'opacity-0 group-hover:opacity-60 hover:!opacity-100 hover:bg-accent',
                      )}
                      aria-label={`Remove ${stream.label} stream`}
                    >
                      <X className="w-3 h-3" />
                    </button>
                  </div>
                )
              })}
            </div>
          </div>

          {/* Separator */}
          <div className="w-px h-6 bg-border" aria-hidden="true" />

          {/* Add Stream Section */}
          <div className="flex items-center gap-3">
            {!showAddStream ? (
              <>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => setShowAddStream(true)}
                  disabled={!isConnected}
                  className="h-7 text-xs border-dashed"
                  aria-label="Add new stream subscription"
                >
                  <Plus className="w-3.5 h-3.5 mr-1.5" />
                  Add
                </Button>

                {/* Quick Add - only show when no streams */}
                {subscribedStreams.length === 0 && (
                  <div className="flex items-center gap-2 pl-2 border-l border-border">
                    <span className="text-[10px] text-muted-foreground">Quick:</span>
                    {PRESET_STREAMS.map((preset) => (
                      <Button
                        key={preset.name}
                        variant="ghost"
                        size="sm"
                        onClick={() => handleAddStream(preset.name, preset.label)}
                        disabled={!isConnected}
                        className="h-6 px-2 text-[10px]"
                      >
                        {preset.label}
                      </Button>
                    ))}
                  </div>
                )}
              </>
            ) : (
              <div className="flex items-center gap-2 px-3 py-1.5 rounded-md bg-muted ring-1 ring-border">
                <Input
                  type="text"
                  value={newStreamName}
                  onChange={(e) => setNewStreamName(e.target.value)}
                  placeholder="__motia.stream-name"
                  autoFocus
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' && newStreamName) handleAddStream(newStreamName)
                    if (e.key === 'Escape') {
                      setShowAddStream(false)
                      setNewStreamName('')
                    }
                  }}
                  className="w-44 h-7 text-xs font-mono border-0 bg-transparent focus-visible:ring-0"
                  aria-label="Stream name"
                />
                <div className="flex items-center gap-1">
                  <Button
                    variant="ghost"
                    size="icon"
                    onClick={() => newStreamName && handleAddStream(newStreamName)}
                    disabled={!newStreamName}
                    className="h-6 w-6 text-emerald-400 hover:text-emerald-300 hover:bg-emerald-500/10"
                    aria-label="Confirm add stream"
                  >
                    <Check className="w-4 h-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    onClick={() => {
                      setShowAddStream(false)
                      setNewStreamName('')
                    }}
                    className="h-6 w-6"
                    aria-label="Cancel add stream"
                  >
                    <X className="w-4 h-4" />
                  </Button>
                </div>
              </div>
            )}
          </div>
        </div>
      </div>

      {/* Message Viewer - Full Width */}
      <div className="flex-1 overflow-hidden">
        {selectedConnectionId ? (
          <MessageViewer
            connectionId={selectedConnectionId}
            onSendMessage={sendMessage}
            isConnected={isConnected}
            streamFilter={activeFilter}
            streamColors={subscribedStreams.reduce(
              (acc, s) => {
                acc[s.name] = STREAM_COLORS[s.colorIndex]
                return acc
              },
              {} as Record<string, (typeof STREAM_COLORS)[0]>,
            )}
          />
        ) : (
          <Empty className="h-full">
            <div className="w-16 h-16 mb-4 rounded-2xl bg-muted border border-border flex items-center justify-center">
              <Wifi className="w-8 h-8 text-muted-foreground animate-pulse" />
            </div>
            <EmptyTitle>Connecting to WebSocket</EmptyTitle>
            <EmptyDescription className="font-mono">ws://localhost:3000</EmptyDescription>
          </Empty>
        )}
      </div>
    </div>
  )
}
