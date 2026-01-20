import { Button, cn, Empty, EmptyDescription, EmptyTitle } from '@motiadev/ui'
import { Check, ChevronDown, ChevronUp, Copy, MessageSquare, Radio } from 'lucide-react'
import React, { useEffect, useRef, useState } from 'react'
import { useWebSocketMessages } from '../hooks/use-websocket-connections'

interface StreamColor {
  bg: string
  border: string
  text: string
  dot: string
}

interface MessageViewerProps {
  connectionId: string
  onSendMessage?: (data: string | object) => void
  isConnected?: boolean
  streamFilter?: string | null
  streamColors?: Record<string, StreamColor>
}

// Copy button with animated feedback
const CopyButton: React.FC<{ text: string }> = ({ text }) => {
  const [copied, setCopied] = useState(false)

  const handleCopy = async (e: React.MouseEvent) => {
    e.stopPropagation()
    try {
      await navigator.clipboard.writeText(text)
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    } catch (err) {
      console.error('Failed to copy:', err)
    }
  }

  return (
    <Button
      variant="ghost"
      size="sm"
      onClick={handleCopy}
      className={cn(
        'h-6 px-2 text-[10px] font-medium uppercase tracking-wider transition-all duration-300',
        copied
          ? 'bg-emerald-500/20 text-emerald-400 hover:bg-emerald-500/30'
          : 'text-muted-foreground hover:text-foreground',
      )}
    >
      <span className={cn('flex items-center gap-1.5 transition-transform', copied && 'scale-105')}>
        {copied ? (
          <>
            <Check className="w-3 h-3" />
            <span>Copied</span>
          </>
        ) : (
          <>
            <Copy className="w-3 h-3" />
            <span>Copy</span>
          </>
        )}
      </span>
    </Button>
  )
}

export const MessageViewer: React.FC<MessageViewerProps> = ({
  connectionId,
  streamFilter = null,
  streamColors = {},
}) => {
  const { messages: allMessages } = useWebSocketMessages(connectionId)
  const [expandedMessages, setExpandedMessages] = useState<Set<string>>(new Set())
  const messagesEndRef = useRef<HTMLDivElement>(null)

  // Filter messages by stream if filter is active
  const messages = streamFilter
    ? allMessages.filter((msg) => {
        if (typeof msg.data === 'object' && msg.data !== null) {
          return (msg.data as Record<string, unknown>).streamName === streamFilter
        }
        return false
      })
    : allMessages

  // Auto-scroll to bottom on new messages
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [])

  const toggleExpanded = (id: string) => {
    setExpandedMessages((prev) => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }

  // Syntax highlight JSON
  const highlightJson = (json: string): React.ReactNode => {
    // Regex patterns for JSON tokens
    const patterns = [
      { regex: /("(?:\\.|[^"\\])*")\s*:/g, className: 'text-purple-400' }, // keys
      { regex: /:\s*("(?:\\.|[^"\\])*")/g, className: 'text-green-400' }, // string values
      { regex: /:\s*(\d+\.?\d*)/g, className: 'text-orange-400' }, // numbers
      { regex: /:\s*(true|false)/g, className: 'text-blue-400' }, // booleans
      { regex: /:\s*(null)/g, className: 'text-zinc-500' }, // null
    ]

    // Split by lines and process each
    const lines = json.split('\n')

    return lines.map((line, lineIdx) => {
      const result: React.ReactNode[] = []
      let lastIndex = 0
      const matches: Array<{
        start: number
        end: number
        text: string
        className: string
      }> = []

      // Find all matches
      patterns.forEach(({ regex, className }) => {
        const re = new RegExp(regex.source, 'g')
        let match: RegExpExecArray | null
        while ((match = re.exec(line)) !== null) {
          const capturedGroup = match[1]
          const groupStart = match.index + match[0].indexOf(capturedGroup)
          matches.push({
            start: groupStart,
            end: groupStart + capturedGroup.length,
            text: capturedGroup,
            className,
          })
        }
      })

      // Sort matches by position
      matches.sort((a, b) => a.start - b.start)

      // Remove overlapping matches
      const filteredMatches: typeof matches = []
      for (const match of matches) {
        if (filteredMatches.length === 0 || match.start >= filteredMatches[filteredMatches.length - 1].end) {
          filteredMatches.push(match)
        }
      }

      // Build result with highlights
      filteredMatches.forEach((match, idx) => {
        if (match.start > lastIndex) {
          result.push(
            <span key={`${lineIdx}-${idx}-pre`} className="text-zinc-300">
              {line.slice(lastIndex, match.start)}
            </span>,
          )
        }
        result.push(
          <span key={`${lineIdx}-${idx}`} className={match.className}>
            {match.text}
          </span>,
        )
        lastIndex = match.end
      })

      if (lastIndex < line.length) {
        result.push(
          <span key={`${lineIdx}-end`} className="text-zinc-300">
            {line.slice(lastIndex)}
          </span>,
        )
      }

      if (result.length === 0) {
        result.push(
          <span key={`${lineIdx}-empty`} className="text-zinc-300">
            {line}
          </span>,
        )
      }

      return (
        <React.Fragment key={lineIdx}>
          {result}
          {lineIdx < lines.length - 1 && '\n'}
        </React.Fragment>
      )
    })
  }

  // Format message data for display
  const formatData = (data: unknown): React.ReactNode => {
    if (typeof data === 'string') {
      // Try to parse as JSON for highlighting
      try {
        const parsed = JSON.parse(data)
        return highlightJson(JSON.stringify(parsed, null, 2))
      } catch {
        return <span className="text-zinc-200">{data}</span>
      }
    }
    try {
      return highlightJson(JSON.stringify(data, null, 2))
    } catch {
      return <span className="text-zinc-200">{String(data)}</span>
    }
  }

  // Get raw JSON string for copying
  const getRawJson = (data: unknown): string => {
    if (typeof data === 'string') {
      try {
        return JSON.stringify(JSON.parse(data), null, 2)
      } catch {
        return data
      }
    }
    try {
      return JSON.stringify(data, null, 2)
    } catch {
      return String(data)
    }
  }

  // Get message type styling with refined colors
  const getMessageStyle = (
    data: string | object,
  ): {
    bg: string
    border: string
    label: string
    labelBg: string
    labelColor: string
    icon: string
  } => {
    if (typeof data === 'object' && data !== null) {
      const obj = data as Record<string, unknown>
      if (obj.type === 'system') {
        return {
          bg: 'bg-gradient-to-br from-amber-950/30 to-amber-950/10',
          border: 'border-amber-700/30',
          label: 'System',
          labelBg: 'bg-amber-500/10',
          labelColor: 'text-amber-400',
          icon: '⚡',
        }
      }
      if (obj.type === 'error') {
        return {
          bg: 'bg-gradient-to-br from-red-950/40 to-red-950/10',
          border: 'border-red-700/30',
          label: 'Error',
          labelBg: 'bg-red-500/10',
          labelColor: 'text-red-400',
          icon: '✕',
        }
      }
      const event = obj.event as Record<string, unknown> | undefined
      if (event?.type === 'sync') {
        return {
          bg: 'bg-gradient-to-br from-violet-950/30 to-violet-950/10',
          border: 'border-violet-700/30',
          label: 'Sync',
          labelBg: 'bg-violet-500/10',
          labelColor: 'text-violet-400',
          icon: '↻',
        }
      }
      if (event?.type === 'create') {
        return {
          bg: 'bg-gradient-to-br from-emerald-950/30 to-emerald-950/10',
          border: 'border-emerald-700/30',
          label: 'Create',
          labelBg: 'bg-emerald-500/10',
          labelColor: 'text-emerald-400',
          icon: '+',
        }
      }
      if (event?.type === 'update') {
        return {
          bg: 'bg-gradient-to-br from-sky-950/30 to-sky-950/10',
          border: 'border-sky-700/30',
          label: 'Update',
          labelBg: 'bg-sky-500/10',
          labelColor: 'text-sky-400',
          icon: '↑',
        }
      }
      if (event?.type === 'delete') {
        return {
          bg: 'bg-gradient-to-br from-rose-950/30 to-rose-950/10',
          border: 'border-rose-700/30',
          label: 'Delete',
          labelBg: 'bg-rose-500/10',
          labelColor: 'text-rose-400',
          icon: '−',
        }
      }
      if (event?.type === 'event') {
        return {
          bg: 'bg-gradient-to-br from-cyan-950/30 to-cyan-950/10',
          border: 'border-cyan-700/30',
          label: 'Event',
          labelBg: 'bg-cyan-500/10',
          labelColor: 'text-cyan-400',
          icon: '◆',
        }
      }
      if (obj.streamName) {
        return {
          bg: 'bg-gradient-to-br from-indigo-950/30 to-indigo-950/10',
          border: 'border-indigo-700/30',
          label: 'Stream',
          labelBg: 'bg-indigo-500/10',
          labelColor: 'text-indigo-400',
          icon: '≋',
        }
      }
    }
    return {
      bg: 'bg-gradient-to-br from-zinc-800/50 to-zinc-900/50',
      border: 'border-zinc-700/50',
      label: 'Data',
      labelBg: 'bg-zinc-700/30',
      labelColor: 'text-zinc-400',
      icon: '{ }',
    }
  }

  return (
    <div className="flex flex-col h-full bg-zinc-950">
      {/* Messages */}
      <div className="flex-1 overflow-y-auto px-5 py-4 space-y-3">
        {messages.map((message, index) => {
          const isSent = message.type === 'sent'
          const msgStreamName: string | null =
            typeof message.data === 'object' && message.data !== null
              ? String((message.data as Record<string, unknown>).streamName ?? '')
              : null
          const streamColor: StreamColor | undefined =
            msgStreamName && streamColors ? streamColors[msgStreamName] : undefined

          let style: {
            bg: string
            border: string
            label: string
            labelBg: string
            labelColor: string
            icon: string
          }

          if (isSent) {
            style = {
              bg: 'bg-gradient-to-br from-blue-950/40 to-blue-950/10',
              border: 'border-blue-700/30',
              label: 'Sent',
              labelBg: 'bg-blue-500/10',
              labelColor: 'text-blue-400',
              icon: '↗',
            }
          } else if (streamColor) {
            style = {
              bg: streamColor.bg,
              border: streamColor.border,
              label: msgStreamName?.replace('__motia.', '').replace(/-/g, ' ') || 'Stream',
              labelBg: streamColor.bg,
              labelColor: streamColor.text,
              icon: '≋',
            }
          } else {
            style = getMessageStyle(message.data)
          }

          const rawJson = getRawJson(message.data)
          const isExpanded = expandedMessages.has(message.id)
          const isLongMessage = rawJson.split('\n').length > 12

          return (
            <div
              key={message.id}
              className={`
                group relative rounded-xl border backdrop-blur-sm
                transition-all duration-300 ease-out
                hover:shadow-lg hover:shadow-black/20
                ${style.bg} ${style.border}
                ${isSent ? 'ml-12' : 'mr-4'}
              `}
              style={{
                animationDelay: `${index * 30}ms`,
              }}
            >
              {/* Card Header */}
              <div className="flex items-center justify-between px-4 py-2.5 border-b border-white/5">
                <div className="flex items-center gap-2.5">
                  {/* Type Badge */}
                  <div className={`flex items-center gap-1.5 px-2 py-0.5 rounded-md ${style.labelBg}`}>
                    <span className={`text-xs ${style.labelColor}`}>{style.icon}</span>
                    <span className={`text-[10px] font-semibold uppercase tracking-wider ${style.labelColor}`}>
                      {style.label}
                    </span>
                  </div>

                  {/* Stream Name */}
                  {typeof message.data === 'object' &&
                    Boolean((message.data as Record<string, unknown>)?.streamName) && (
                      <span className="text-[10px] text-zinc-500 font-mono px-1.5 py-0.5 bg-zinc-800/50 rounded">
                        {String((message.data as Record<string, unknown>).streamName)}
                      </span>
                    )}
                </div>

                <div className="flex items-center gap-2">
                  {/* Copy Button */}
                  <CopyButton text={rawJson} />

                  {/* Timestamp */}
                  <span className="text-[10px] text-zinc-600 font-mono tabular-nums">
                    {new Date(message.timestamp).toLocaleTimeString('en-US', {
                      hour12: false,
                      hour: '2-digit',
                      minute: '2-digit',
                      second: '2-digit',
                    })}
                  </span>
                </div>
              </div>

              {/* JSON Content */}
              <div className="relative">
                <pre
                  className={`
                    px-4 py-3 text-[11px] leading-relaxed font-mono
                    whitespace-pre-wrap break-words overflow-auto
                    ${isLongMessage && !isExpanded ? 'max-h-64' : 'max-h-[600px]'}
                    transition-all duration-300
                  `}
                >
                  {formatData(message.data)}
                </pre>

                {/* Expand/Collapse for long messages */}
                {isLongMessage && (
                  <div
                    className={`
                    absolute bottom-0 left-0 right-0
                    ${
                      !isExpanded
                        ? 'bg-gradient-to-t from-zinc-900/95 via-zinc-900/80 to-transparent pt-12 pb-2'
                        : 'pb-2'
                    }
                  `}
                  >
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => toggleExpanded(message.id)}
                      className="w-full h-7 text-[10px] font-medium text-muted-foreground hover:text-foreground"
                    >
                      {isExpanded ? (
                        <>
                          <ChevronUp className="w-3 h-3 mr-1.5" />
                          Collapse
                        </>
                      ) : (
                        <>
                          <ChevronDown className="w-3 h-3 mr-1.5" />
                          Expand ({rawJson.split('\n').length} lines)
                        </>
                      )}
                    </Button>
                  </div>
                )}
              </div>
            </div>
          )
        })}

        {/* Empty State */}
        {messages.length === 0 && (
          <Empty className="h-64">
            <div
              className={cn(
                'w-14 h-14 mb-4 rounded-xl border flex items-center justify-center',
                streamFilter && streamColors?.[streamFilter]
                  ? `${streamColors[streamFilter].bg} ${streamColors[streamFilter].border}`
                  : 'bg-muted/50 border-border',
              )}
            >
              {streamFilter ? (
                <Radio className={cn('w-7 h-7', streamColors?.[streamFilter]?.text || 'text-muted-foreground')} />
              ) : (
                <MessageSquare className="w-7 h-7 text-muted-foreground" />
              )}
            </div>
            <EmptyTitle>{streamFilter ? 'No messages in this stream' : 'No messages yet'}</EmptyTitle>
            <EmptyDescription>
              {streamFilter
                ? 'Messages will appear here when data flows through'
                : 'Add a stream to start receiving messages'}
            </EmptyDescription>
          </Empty>
        )}
        <div ref={messagesEndRef} />
      </div>
    </div>
  )
}
