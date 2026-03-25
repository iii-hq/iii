import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { Activity, AlertTriangle, Box, Inbox, ListOrdered, RefreshCw, Users, X } from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { z } from 'zod'
import { dlqTopicsQuery, queueDetailQuery, queuesQuery } from '@/api/queries'
import { publishToQueue } from '@/api/queues/queues'
import { QueueDlqTab } from '@/components/queues/QueueDlqTab'
import { QueueOverviewTab } from '@/components/queues/QueueOverviewTab'
import { Badge, Button } from '@/components/ui/card'
import { EmptyState } from '@/components/ui/empty-state'
import { PageHeader } from '@/components/ui/page-header'
import { SearchBar } from '@/components/ui/search-bar'
import { Skeleton } from '@/components/ui/skeleton'
import { useResizableSplitPane } from '@/hooks/useResizableSplitPane'

const searchSchema = z.object({
  topic: z.string().optional(),
  tab: z.enum(['overview', 'dead-letters']).optional().default('overview'),
})

export const Route = createFileRoute('/queues')({
  validateSearch: searchSchema,
  component: QueuesPage,
})

type Tab = 'overview' | 'dead-letters'

function QueuesPage() {
  const navigate = useNavigate({ from: '/queues' })
  const { topic: selectedTopic, tab: activeTab } = Route.useSearch()
  const [search, setSearch] = useState('')
  const [publishError, setPublishError] = useState<string | null>(null)
  const containerRef = useRef<HTMLDivElement>(null)

  const queryClient = useQueryClient()

  // Queries
  const {
    data: queuesData,
    isLoading,
    refetch,
    isFetching,
  } = useQuery({ ...queuesQuery, refetchInterval: 3000 })
  const { data: detail } = useQuery({
    ...queueDetailQuery(selectedTopic ?? ''),
    refetchInterval: selectedTopic ? 2000 : false,
  })
  const { data: dlqData } = useQuery({ ...dlqTopicsQuery, refetchInterval: 5000 })

  // Resizable split pane
  const { rightPanelWidth, isResizing, startResize } = useResizableSplitPane({
    rightPanelOpen: !!selectedTopic,
    containerRef,
  })

  // Publish mutation
  const publishMutation = useMutation({
    mutationFn: async (json: string) => {
      if (!selectedTopic) return
      try {
        const parsed = JSON.parse(json)
        await publishToQueue(selectedTopic, parsed)
      } catch (e) {
        throw e instanceof SyntaxError ? new Error('Invalid JSON') : e
      }
    },
    onSuccess: () => {
      setPublishError(null)
      queryClient.invalidateQueries({ queryKey: ['queues'] })
    },
    onError: (e: Error) => setPublishError(e.message),
  })

  // Data
  const queues = queuesData?.queues ?? []
  const dlqTopics = dlqData?.topics ?? []

  const filtered = useMemo(
    () =>
      queues
        .filter((q) => q.name.toLowerCase().includes(search.toLowerCase()))
        .sort((a, b) => a.name.localeCompare(b.name)),
    [queues, search],
  )

  const totalSubscribers = useMemo(
    () => queues.reduce((sum, q) => sum + q.subscriber_count, 0),
    [queues],
  )

  // DLQ badge lookup
  const dlqCountMap = useMemo(() => {
    const map = new Map<string, number>()
    for (const t of dlqTopics) {
      if (t.message_count > 0) map.set(t.topic, t.message_count)
    }
    return map
  }, [dlqTopics])

  const selectedDlqEntry = useMemo(
    () => dlqTopics.find((t) => t.topic === selectedTopic),
    [dlqTopics, selectedTopic],
  )

  // Stale topic detection — only after data loads
  useEffect(() => {
    if (isLoading || !selectedTopic) return
    const exists = queues.some((q) => q.name === selectedTopic)
    if (!exists) {
      navigate({ search: { topic: undefined, tab: 'overview' }, replace: true })
    }
  }, [isLoading, selectedTopic, queues, navigate])

  // Navigation helpers
  const selectQueue = useCallback(
    (topic: string | undefined) => {
      if (topic === selectedTopic) {
        navigate({ search: { topic: undefined, tab: 'overview' }, replace: true })
      } else {
        navigate({ search: { topic, tab: 'overview' }, replace: true })
      }
      setPublishError(null)
    },
    [selectedTopic, navigate],
  )

  const setActiveTab = useCallback(
    (tab: Tab) => {
      navigate({ search: (prev) => ({ ...prev, tab }), replace: true })
    },
    [navigate],
  )

  // Keyboard navigation
  const [highlightedIndex, setHighlightedIndex] = useState(-1)

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      const el = document.activeElement
      if (
        el &&
        (el.tagName === 'INPUT' ||
          el.tagName === 'TEXTAREA' ||
          (el as HTMLElement).isContentEditable)
      )
        return
      if (e.metaKey || e.ctrlKey || e.altKey) return

      switch (e.key) {
        case 'j': {
          e.preventDefault()
          setHighlightedIndex((prev) => Math.min(prev + 1, filtered.length - 1))
          break
        }
        case 'k': {
          e.preventDefault()
          setHighlightedIndex((prev) => Math.max(prev - 1, 0))
          break
        }
        case 'Enter': {
          if (highlightedIndex >= 0 && highlightedIndex < filtered.length) {
            e.preventDefault()
            selectQueue(filtered[highlightedIndex].name)
          }
          break
        }
        case 'Escape': {
          if (selectedTopic) {
            e.preventDefault()
            selectQueue(undefined)
          } else if (search) {
            e.preventDefault()
            setSearch('')
          }
          break
        }
        case '1': {
          if (selectedTopic) {
            e.preventDefault()
            setActiveTab('overview')
          }
          break
        }
        case '2': {
          if (selectedTopic) {
            e.preventDefault()
            setActiveTab('dead-letters')
          }
          break
        }
      }
    }

    document.addEventListener('keydown', handleKeyDown)
    return () => document.removeEventListener('keydown', handleKeyDown)
  }, [filtered, highlightedIndex, selectedTopic, search, selectQueue, setActiveTab])

  // DLQ count for badge: use the higher of detail stats vs dlqTopicsQuery
  // to avoid badge disappearing while detail stats are stale or loading
  const getDlqCount = useCallback(
    (queueName: string) => {
      const dlqTopicCount = dlqCountMap.get(queueName) ?? 0
      if (queueName === selectedTopic && detail?.stats) {
        return Math.max(detail.stats.dlq_depth, dlqTopicCount)
      }
      return dlqTopicCount
    },
    [selectedTopic, detail, dlqCountMap],
  )

  return (
    <div className="flex flex-col h-full bg-background text-foreground">
      {/* Header */}
      <PageHeader
        icon={Inbox}
        title="Queues"
        actions={
          <Button
            variant="ghost"
            size="sm"
            onClick={() => refetch()}
            disabled={isFetching}
            className="h-7 text-xs"
          >
            <RefreshCw className={`w-3.5 h-3.5 mr-1.5 ${isFetching ? 'animate-spin' : ''}`} />
            Refresh
          </Button>
        }
      >
        <Badge variant="default" className="gap-1 text-[10px] md:text-xs">
          <Activity className="w-2.5 h-2.5 md:w-3 md:h-3" />
          {queues.length} topics
        </Badge>
        <span className="font-sans text-sm text-secondary hidden md:inline">
          {totalSubscribers} subscribers
        </span>
      </PageHeader>

      {/* Search Bar */}
      <SearchBar value={search} onChange={setSearch} placeholder="Filter queues..." />

      {/* Main Content — resizable split */}
      <div ref={containerRef} className="flex-1 flex overflow-hidden">
        {/* Queue List */}
        <div className="flex-1 overflow-y-auto min-w-[280px]">
          {isLoading ? (
            <div className="p-4 space-y-3">
              {(['qsk-0', 'qsk-1', 'qsk-2', 'qsk-3', 'qsk-4'] as const).map((sk) => (
                <div key={sk} className="flex items-center gap-4 px-4 py-3">
                  <Skeleton className="h-4 w-4 rounded" />
                  <Skeleton className="h-4 flex-1" />
                  <Skeleton className="h-4 w-20" />
                  <Skeleton className="h-4 w-12" />
                </div>
              ))}
            </div>
          ) : filtered.length === 0 ? (
            search ? (
              <EmptyState
                icon={ListOrdered}
                title="No queues match your filter"
                description="Try a different search term"
              />
            ) : (
              <EmptyState
                icon={ListOrdered}
                title="No queues found"
                description="Queues are created when functions use named queues"
              />
            )
          ) : (
            <table className="w-full">
              <thead>
                <tr className="border-b border-border">
                  <th className="text-left py-3 px-4 md:px-6 font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted">
                    Topic
                  </th>
                  <th className="text-left py-3 px-4 font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted">
                    Broker
                  </th>
                  <th className="text-right py-3 px-4 font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted">
                    DLQ
                  </th>
                  <th className="text-right py-3 px-4 md:px-6 font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted">
                    Subscribers
                  </th>
                </tr>
              </thead>
              <tbody>
                {filtered.map((q, idx) => {
                  const isSelected = q.name === selectedTopic
                  const isHighlighted = idx === highlightedIndex
                  const dlqCount = getDlqCount(q.name)

                  return (
                    <tr
                      key={q.name}
                      onClick={() => selectQueue(q.name)}
                      className={`border-b border-border/50 cursor-pointer transition-colors ${
                        isSelected
                          ? 'bg-yellow/5 border-l-2 border-l-yellow'
                          : isHighlighted
                            ? 'bg-dark-gray/40 border-l-2 border-l-muted'
                            : 'hover:bg-dark-gray/30 border-l-2 border-l-transparent'
                      }`}
                    >
                      <td className="px-4 md:px-6 py-3">
                        <div className="flex items-center gap-2.5">
                          <Box
                            className={`w-3.5 h-3.5 shrink-0 ${isSelected ? 'text-yellow' : 'text-muted'}`}
                          />
                          <span className="font-mono text-[13px] text-foreground">{q.name}</span>
                        </div>
                      </td>
                      <td className="px-4 py-3">
                        <Badge variant="outline" className="text-[10px]">
                          {q.broker_type}
                        </Badge>
                      </td>
                      <td className="px-4 py-3 text-right">
                        {dlqCount > 0 && (
                          <Badge variant="error" className="gap-1 text-[10px]">
                            <AlertTriangle className="w-2.5 h-2.5" />
                            {dlqCount}
                          </Badge>
                        )}
                      </td>
                      <td className="px-4 md:px-6 py-3 text-right">
                        <span className="inline-flex items-center gap-1.5 font-mono text-[13px] text-muted">
                          <Users className="w-3 h-3" />
                          {q.subscriber_count}
                        </span>
                      </td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
          )}
        </div>

        {/* Resize Handle + Detail Panel */}
        {selectedTopic && (
          <>
            {/* Resize handle: button avoids non-focusable separator ARIA requirements */}
            <button
              type="button"
              aria-label="Resize queue detail panel"
              className="w-[3px] p-0 m-0 border-0 bg-transparent hover:bg-border cursor-col-resize shrink-0 hidden lg:block transition-colors min-h-0 self-stretch rounded-none"
              style={isResizing ? { backgroundColor: 'var(--accent)', opacity: 0.5 } : undefined}
              onMouseDown={startResize}
            />

            {/* Detail Panel */}
            <div
              className="flex flex-col shrink-0 animate-panel-in bg-dark-gray/10 border-l border-border"
              style={{ width: `${rightPanelWidth}px` }}
            >
              {/* Panel Header + Tabs */}
              <div className="bg-dark-gray/30 border-b border-border">
                <div className="flex items-center justify-between px-4 py-2">
                  <div className="flex items-center gap-2 min-w-0">
                    <Box className="w-4 h-4 text-yellow shrink-0" />
                    <h2 className="text-xs font-medium tracking-wider uppercase text-foreground truncate">
                      {selectedTopic}
                    </h2>
                  </div>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => selectQueue(undefined)}
                    className="h-6 w-6 p-0 shrink-0"
                  >
                    <X className="w-3.5 h-3.5" />
                  </Button>
                </div>
                <div className="flex px-1" role="tablist">
                  <button
                    type="button"
                    role="tab"
                    aria-selected={activeTab === 'overview'}
                    onClick={() => setActiveTab('overview')}
                    className={`px-4 py-2.5 font-sans text-xs uppercase tracking-[0.04em] transition-colors ${
                      activeTab === 'overview'
                        ? 'font-semibold text-foreground border-b-2 border-accent'
                        : 'text-secondary hover:text-foreground'
                    }`}
                  >
                    Overview
                  </button>
                  <button
                    type="button"
                    role="tab"
                    aria-selected={activeTab === 'dead-letters'}
                    onClick={() => setActiveTab('dead-letters')}
                    className={`px-4 py-2.5 font-sans text-xs uppercase tracking-[0.04em] transition-colors flex items-center gap-2 ${
                      activeTab === 'dead-letters'
                        ? 'font-semibold text-foreground border-b-2 border-accent'
                        : 'text-secondary hover:text-foreground'
                    }`}
                  >
                    Dead Letters
                    {getDlqCount(selectedTopic) > 0 && (
                      <Badge variant="error" className="text-[9px] px-1.5 py-0">
                        {getDlqCount(selectedTopic)}
                      </Badge>
                    )}
                  </button>
                </div>
              </div>

              {/* Tab Content */}
              {activeTab === 'overview' ? (
                <QueueOverviewTab
                  stats={detail?.stats}
                  onPublish={(json) => publishMutation.mutate(json)}
                  isPublishing={publishMutation.isPending}
                  publishError={publishError}
                  onClearPublishError={() => setPublishError(null)}
                />
              ) : (
                <QueueDlqTab topic={selectedTopic} dlqEntry={selectedDlqEntry} />
              )}
            </div>
          </>
        )}
      </div>
    </div>
  )
}
