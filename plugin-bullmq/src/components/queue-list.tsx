import { cn, Input } from '@motiadev/ui'
import { AlertTriangle, Layers, Pause, Search, Skull, X } from 'lucide-react'
import { memo, useMemo } from 'react'
import { useBullMQStore } from '../stores/use-bullmq-store'
import type { QueueInfo } from '../types/queue'

type QueueItemProps = {
  queue: QueueInfo
  isSelected: boolean
  onClick: () => void
}

const QueueItem = memo<QueueItemProps>(({ queue, isSelected, onClick }) => {
  const totalJobs = queue.stats.waiting + queue.stats.active + queue.stats.delayed + queue.stats.prioritized
  const hasFailedJobs = queue.stats.failed > 0

  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        'w-full text-left p-3 transition-colors border-b border-border',
        isSelected ? 'bg-muted-foreground/10' : 'hover:bg-muted/70',
      )}
    >
      <div className="flex items-center gap-2 mb-1">
        {queue.isDLQ ? (
          <Skull className="w-4 h-4 text-destructive" />
        ) : (
          <Layers className="w-4 h-4 text-muted-foreground" />
        )}
        <span className="font-semibold text-sm truncate flex-1">{queue.displayName}</span>
        {queue.isPaused && <Pause className="w-3 h-3 text-yellow-500" />}
        {hasFailedJobs && <AlertTriangle className="w-3 h-3 text-destructive" />}
      </div>

      <div className="grid grid-cols-4 gap-1 text-xs">
        <div className="flex flex-col">
          <span className="text-muted-foreground">Wait</span>
          <span className={cn('font-mono', queue.stats.waiting > 0 && 'text-blue-500')}>{queue.stats.waiting}</span>
        </div>
        <div className="flex flex-col">
          <span className="text-muted-foreground">Active</span>
          <span className={cn('font-mono', queue.stats.active > 0 && 'text-yellow-500')}>{queue.stats.active}</span>
        </div>
        <div className="flex flex-col">
          <span className="text-muted-foreground">Done</span>
          <span className={cn('font-mono', queue.stats.completed > 0 && 'text-green-500')}>
            {queue.stats.completed}
          </span>
        </div>
        <div className="flex flex-col">
          <span className="text-muted-foreground">Failed</span>
          <span className={cn('font-mono', queue.stats.failed > 0 && 'text-destructive')}>{queue.stats.failed}</span>
        </div>
      </div>

      {(queue.stats.delayed > 0 || totalJobs > 0) && (
        <div className="flex gap-2 mt-1 text-xs text-muted-foreground">
          {queue.stats.delayed > 0 && <span>{queue.stats.delayed} delayed</span>}
          {totalJobs > 0 && <span className="ml-auto">{totalJobs} pending</span>}
        </div>
      )}
    </button>
  )
})
QueueItem.displayName = 'QueueItem'

export const QueueList = memo(() => {
  const queues = useBullMQStore((state) => state.queues)
  const selectedQueue = useBullMQStore((state) => state.selectedQueue)
  const setSelectedQueue = useBullMQStore((state) => state.setSelectedQueue)
  const searchQuery = useBullMQStore((state) => state.searchQuery)
  const setSearchQuery = useBullMQStore((state) => state.setSearchQuery)

  const filteredQueues = useMemo(() => {
    if (!searchQuery) return queues
    const query = searchQuery.toLowerCase()
    return queues.filter((q) => q.name.toLowerCase().includes(query) || q.displayName.toLowerCase().includes(query))
  }, [queues, searchQuery])

  const regularQueues = useMemo(() => filteredQueues.filter((q) => !q.isDLQ), [filteredQueues])
  const dlqQueues = useMemo(() => filteredQueues.filter((q) => q.isDLQ), [filteredQueues])

  return (
    <div className="flex flex-col h-full">
      <div className="p-2 border-b border-border">
        <div className="relative">
          <Input
            variant="shade"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="px-9! font-medium text-sm"
            placeholder="Search queues..."
          />
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted-foreground/50" />
          {searchQuery && (
            <X
              className="cursor-pointer absolute right-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted-foreground/50 hover:text-muted-foreground"
              onClick={() => setSearchQuery('')}
            />
          )}
        </div>
      </div>

      <div className="flex-1 overflow-auto">
        {regularQueues.length > 0 && (
          <div>
            <div className="px-3 py-2 text-xs font-semibold text-muted-foreground uppercase tracking-wider bg-muted/30">
              Queues ({regularQueues.length})
            </div>
            {regularQueues.map((queue) => (
              <QueueItem
                key={queue.name}
                queue={queue}
                isSelected={selectedQueue?.name === queue.name}
                onClick={() => setSelectedQueue(queue)}
              />
            ))}
          </div>
        )}

        {dlqQueues.length > 0 && (
          <div>
            <div className="px-3 py-2 text-xs font-semibold text-muted-foreground uppercase tracking-wider bg-destructive/10">
              Dead Letter Queues ({dlqQueues.length})
            </div>
            {dlqQueues.map((queue) => (
              <QueueItem
                key={queue.name}
                queue={queue}
                isSelected={selectedQueue?.name === queue.name}
                onClick={() => setSelectedQueue(queue)}
              />
            ))}
          </div>
        )}

        {filteredQueues.length === 0 && (
          <div className="p-4 text-center text-muted-foreground text-sm">
            {searchQuery ? 'No queues match your search' : 'No queues found'}
          </div>
        )}
      </div>
    </div>
  )
})
QueueList.displayName = 'QueueList'
