import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  AlertTriangle,
  CheckCircle,
  ChevronRight,
  Inbox,
  RotateCcw,
  Trash2,
  XCircle,
} from 'lucide-react'
import { useState } from 'react'
import { dlqMessagesQuery } from '@/api/queries'
import type { DlqMessage, DlqTopic } from '@/api/queues/queues'
import { discardMessage, redriveDlq, redriveMessage } from '@/api/queues/queues'
import { Badge, Button } from '@/components/ui/card'
import { EmptyState } from '@/components/ui/empty-state'
import { JsonViewer } from '@/components/ui/json-viewer'
import { Skeleton } from '@/components/ui/skeleton'
import { Tooltip } from '@/components/ui/tooltip'

function extractErrorMessage(error: string): string {
  // Extract human-readable message from Rust ErrorBody format:
  // 'ErrorBody { code: "invocation_failed", message: "Simulated failure", stacktrace: ... }'
  const msgMatch = error.match(/message:\s*"([^"]*)"/)
  if (msgMatch) return msgMatch[1]
  // Fallback: if it's a plain string, return as-is (truncated)
  return error.length > 100 ? `${error.slice(0, 100)}...` : error
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}

interface QueueDlqTabProps {
  topic: string
  dlqEntry: DlqTopic | undefined
}

export function QueueDlqTab({ topic, dlqEntry }: QueueDlqTabProps) {
  const [expandedMessage, setExpandedMessage] = useState<string | null>(null)
  const [confirmDiscard, setConfirmDiscard] = useState<string | null>(null)
  const [redriveSuccess, setRedriveSuccess] = useState<string | null>(null)
  const [redriveError, setRedriveError] = useState<string | null>(null)
  const [confirmRedriveAll, setConfirmRedriveAll] = useState(false)

  const queryClient = useQueryClient()
  const { data: messagesData, isLoading } = useQuery({
    ...dlqMessagesQuery(topic),
    refetchInterval: 3000,
  })

  const messages = messagesData?.messages ?? []
  const expandedMsg = messages.find((m) => m.id === expandedMessage)

  const bulkRedriveMutation = useMutation({
    mutationFn: () => redriveDlq(topic),
    onSuccess: (data) => {
      setRedriveError(null)
      setRedriveSuccess(`Redrived ${data.redriven} messages to ${data.queue}`)
      setExpandedMessage(null)
      queryClient.invalidateQueries({ queryKey: ['dlq-topics'] })
      queryClient.invalidateQueries({ queryKey: ['dlq-messages'] })
      queryClient.invalidateQueries({ queryKey: ['queue-detail', topic] })
      setTimeout(() => setRedriveSuccess(null), 4000)
    },
    onError: (e: Error) => {
      setRedriveError(`Redrive failed: ${e.message}`)
      setTimeout(() => setRedriveError(null), 6000)
    },
  })

  const messageRedriveMutation = useMutation({
    mutationFn: (messageId: string) => redriveMessage(topic, messageId),
    onMutate: async (messageId) => {
      await queryClient.cancelQueries({ queryKey: ['dlq-messages', topic, 0, 50] })
      const prev = queryClient.getQueryData<{ topic: string; messages: DlqMessage[] }>([
        'dlq-messages',
        topic,
        0,
        50,
      ])
      if (prev) {
        queryClient.setQueryData(['dlq-messages', topic, 0, 50], {
          ...prev,
          messages: prev.messages.filter((m) => m.id !== messageId),
        })
      }
      if (expandedMessage === messageId) setExpandedMessage(null)
      return { prev }
    },
    onError: (_err, _messageId, context) => {
      if (context?.prev) {
        queryClient.setQueryData(['dlq-messages', topic, 0, 50], context.prev)
      }
      setRedriveError('Message redrive failed')
      setTimeout(() => setRedriveError(null), 4000)
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ['dlq-messages'] })
      queryClient.invalidateQueries({ queryKey: ['dlq-topics'] })
      queryClient.invalidateQueries({ queryKey: ['queue-detail', topic] })
    },
  })

  const discardMutation = useMutation({
    mutationFn: (messageId: string) => discardMessage(topic, messageId),
    onMutate: async (messageId) => {
      await queryClient.cancelQueries({ queryKey: ['dlq-messages', topic, 0, 50] })
      const prev = queryClient.getQueryData<{ topic: string; messages: DlqMessage[] }>([
        'dlq-messages',
        topic,
        0,
        50,
      ])
      if (prev) {
        queryClient.setQueryData(['dlq-messages', topic, 0, 50], {
          ...prev,
          messages: prev.messages.filter((m) => m.id !== messageId),
        })
      }
      if (expandedMessage === messageId) setExpandedMessage(null)
      return { prev }
    },
    onError: (_err, _messageId, context) => {
      if (context?.prev) {
        queryClient.setQueryData(['dlq-messages', topic, 0, 50], context.prev)
      }
      setRedriveError('Discard failed')
      setTimeout(() => setRedriveError(null), 4000)
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ['dlq-messages'] })
      queryClient.invalidateQueries({ queryKey: ['dlq-topics'] })
      queryClient.invalidateQueries({ queryKey: ['queue-detail', topic] })
    },
  })

  // Celebration only when DLQ entry explicitly has 0 messages (after redrive)
  // NOT when messages haven't loaded yet or API returned empty despite having a count
  const hasDlqEntry = dlqEntry !== undefined
  const isDlqCleared = hasDlqEntry && dlqEntry.message_count === 0

  if (isLoading) {
    return (
      <div className="p-4 space-y-3">
        {(['dlq-sk-0', 'dlq-sk-1', 'dlq-sk-2', 'dlq-sk-3', 'dlq-sk-4'] as const).map((sk) => (
          <div key={sk} className="flex items-center gap-4 px-4 py-3">
            <Skeleton className="h-4 w-4 rounded" />
            <Skeleton className="h-4 flex-1" />
            <Skeleton className="h-4 w-24" />
            <Skeleton className="h-4 w-8" />
          </div>
        ))}
      </div>
    )
  }

  return (
    <div className="flex-1 flex flex-col overflow-hidden">
      {/* Header with redrive + feedback */}
      <div className="px-4 py-3 border-b border-border flex items-center justify-between bg-dark-gray/30">
        <div className="flex items-center gap-2 min-w-0">
          {redriveSuccess && (
            <div className="flex items-center gap-1.5 text-[10px] text-success font-mono animate-trace-flash">
              <CheckCircle className="w-3 h-3" />
              {redriveSuccess}
            </div>
          )}
          {redriveError && (
            <div className="flex items-center gap-1.5 text-[10px] text-error font-mono">
              <XCircle className="w-3 h-3" />
              {redriveError}
            </div>
          )}
        </div>
        {confirmRedriveAll ? (
          <div className="flex items-center gap-2 shrink-0">
            <span className="text-xs text-muted font-mono">
              Redrive all {messages.length} messages?
            </span>
            <Button
              variant="accent"
              size="sm"
              onClick={() => {
                bulkRedriveMutation.mutate()
                setConfirmRedriveAll(false)
              }}
              disabled={bulkRedriveMutation.isPending}
              className="h-7 text-xs gap-1.5"
            >
              Confirm
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setConfirmRedriveAll(false)}
              className="h-7 text-xs"
            >
              Cancel
            </Button>
          </div>
        ) : (
          <Button
            variant="accent"
            size="sm"
            onClick={() => setConfirmRedriveAll(true)}
            disabled={bulkRedriveMutation.isPending || messages.length === 0}
            className="gap-1.5 shrink-0"
          >
            <RotateCcw
              className={`w-3 h-3 ${bulkRedriveMutation.isPending ? 'animate-spin' : ''}`}
            />
            Redrive All
          </Button>
        )}
      </div>

      {/* Message list or empty state */}
      <div className="flex-1 overflow-y-auto overflow-x-hidden">
        {messages.length === 0 ? (
          isDlqCleared ? (
            <EmptyState
              icon={CheckCircle}
              variant="success"
              title="All clear"
              description="No dead letters for this queue"
            />
          ) : (
            <EmptyState
              icon={AlertTriangle}
              title="No failed messages"
              description="Messages that fail processing will appear here"
            />
          )
        ) : (
          <div className="divide-y divide-border/50">
            {messages.map((m) => {
              const isExpanded = m.id === expandedMessage
              const errorMsg = extractErrorMessage(m.error)

              return (
                <div key={m.id} className="group/row">
                  {/* Row */}
                  <div
                    className={`relative flex items-center transition-colors ${
                      isExpanded
                        ? 'bg-error/5 border-l-2 border-l-error'
                        : 'hover:bg-dark-gray/30 border-l-2 border-l-transparent'
                    }`}
                  >
                    <button
                      type="button"
                      onClick={() => setExpandedMessage(isExpanded ? null : m.id)}
                      className="flex-1 flex items-center gap-3 px-4 py-3 cursor-pointer text-left min-w-0"
                    >
                      <ChevronRight
                        className={`w-3.5 h-3.5 shrink-0 text-muted transition-transform ${isExpanded ? 'rotate-90' : ''}`}
                      />
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2">
                          <span className="font-mono text-[13px] text-foreground truncate">
                            {m.id}
                          </span>
                          <Badge
                            variant={m.retries > 2 ? 'warning' : 'default'}
                            className="text-[10px] shrink-0"
                          >
                            {m.retries}x
                          </Badge>
                          <span className="font-mono text-[11px] text-muted shrink-0">
                            {formatBytes(m.size_bytes)}
                          </span>
                        </div>
                        <div className="mt-0.5 text-xs text-error truncate">{errorMsg}</div>
                      </div>
                    </button>
                    {/* Inline row actions — visible on hover */}
                    <div className="flex items-center gap-1 pr-3 opacity-0 group-hover/row:opacity-100 transition-opacity shrink-0">
                      <Tooltip label="Redrive">
                        <button
                          type="button"
                          aria-label="Redrive message"
                          onClick={(e) => {
                            e.stopPropagation()
                            messageRedriveMutation.mutate(m.id)
                          }}
                          disabled={messageRedriveMutation.isPending}
                          className="p-1.5 rounded-[var(--radius-md)] text-muted hover:text-foreground hover:bg-hover transition-colors cursor-pointer"
                        >
                          <RotateCcw className="w-3.5 h-3.5" />
                        </button>
                      </Tooltip>
                      {confirmDiscard === m.id ? (
                        <div className="flex items-center gap-1 bg-error/10 border border-error/20 rounded-[var(--radius-md)] px-2 py-0.5">
                          <span className="text-[10px] text-error font-sans">Delete?</span>
                          <button
                            type="button"
                            onClick={(e) => {
                              e.stopPropagation()
                              discardMutation.mutate(m.id)
                              setConfirmDiscard(null)
                            }}
                            className="px-1.5 py-0.5 text-[10px] font-sans font-medium text-white bg-error rounded-[var(--radius-sm)] hover:bg-error/80 cursor-pointer"
                          >
                            Yes
                          </button>
                          <button
                            type="button"
                            onClick={(e) => {
                              e.stopPropagation()
                              setConfirmDiscard(null)
                            }}
                            className="px-1.5 py-0.5 text-[10px] font-sans text-muted hover:text-foreground cursor-pointer"
                          >
                            No
                          </button>
                        </div>
                      ) : (
                        <Tooltip label="Discard">
                          <button
                            type="button"
                            aria-label="Discard message"
                            onClick={(e) => {
                              e.stopPropagation()
                              setConfirmDiscard(m.id)
                            }}
                            className="p-1.5 rounded-[var(--radius-md)] text-muted hover:text-error hover:bg-error/10 transition-colors cursor-pointer"
                          >
                            <Trash2 className="w-3.5 h-3.5" />
                          </button>
                        </Tooltip>
                      )}
                    </div>
                  </div>
                  {/* Expanded detail */}
                  {isExpanded && expandedMsg && (
                    <div className="bg-dark-gray/10 border-l-2 border-l-error px-5 py-4 space-y-4">
                      {/* Actions bar */}
                      <div className="flex items-center gap-2">
                        <Button
                          variant="outline"
                          size="sm"
                          onClick={() => messageRedriveMutation.mutate(m.id)}
                          disabled={messageRedriveMutation.isPending}
                          className="h-7 text-xs gap-1.5"
                        >
                          <RotateCcw
                            className={`w-3 h-3 ${messageRedriveMutation.isPending ? 'animate-spin' : ''}`}
                          />
                          Redrive
                        </Button>
                        {confirmDiscard === m.id ? (
                          <>
                            <Button
                              variant="destructive"
                              size="sm"
                              onClick={() => {
                                discardMutation.mutate(m.id)
                                setConfirmDiscard(null)
                              }}
                              disabled={discardMutation.isPending}
                              className="h-7 text-xs gap-1.5"
                            >
                              <Trash2 className="w-3 h-3" />
                              Confirm Delete
                            </Button>
                            <Button
                              variant="ghost"
                              size="sm"
                              onClick={() => setConfirmDiscard(null)}
                              className="h-7 text-xs"
                            >
                              Cancel
                            </Button>
                          </>
                        ) : (
                          <Button
                            variant="ghost"
                            size="sm"
                            onClick={() => setConfirmDiscard(m.id)}
                            className="h-7 text-xs gap-1.5 text-error hover:text-error hover:bg-error/10"
                          >
                            <Trash2 className="w-3 h-3" />
                            Discard
                          </Button>
                        )}
                        <span className="ml-auto font-mono text-[11px] text-muted">
                          {new Date(expandedMsg.failed_at * 1000).toLocaleString()} ·{' '}
                          {formatBytes(expandedMsg.size_bytes)}
                        </span>
                      </div>

                      {/* Error */}
                      <div>
                        <div className="flex items-center gap-1.5 mb-1.5">
                          <AlertTriangle className="w-3 h-3 text-error" />
                          <span className="font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted">
                            Error
                          </span>
                        </div>
                        <div className="bg-error/5 border border-error/20 rounded-[var(--radius-lg)] p-3 font-mono text-[13px] text-error break-all leading-relaxed">
                          {expandedMsg.error}
                        </div>
                      </div>

                      {/* Payload */}
                      <div>
                        <div className="flex items-center gap-1.5 mb-1.5">
                          <Inbox className="w-3 h-3 text-muted" />
                          <span className="font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted">
                            Payload
                          </span>
                        </div>
                        <div className="rounded-[var(--radius-lg)] bg-elevated border border-border-subtle p-3 overflow-x-auto max-h-64 overflow-y-auto">
                          <JsonViewer data={expandedMsg.payload} collapsed={false} maxDepth={6} />
                        </div>
                      </div>
                    </div>
                  )}
                </div>
              )
            })}
          </div>
        )}
      </div>
    </div>
  )
}
