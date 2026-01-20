import {
  Button,
  cn,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
  Tabs,
  TabsList,
  TabsTrigger,
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@motiadev/ui'
import { useQueryClient } from '@tanstack/react-query'
import { MoreVertical, Pause, Play, RefreshCw, Trash } from 'lucide-react'
import { memo, useCallback } from 'react'
import { useQueues } from '../hooks/use-queues'
import { useBullMQStore } from '../stores/use-bullmq-store'
import type { JobStatus } from '../types/queue'
import { JobsTable } from './jobs-table'

const STATUS_TABS: { value: JobStatus; label: string }[] = [
  { value: 'waiting', label: 'Waiting' },
  { value: 'active', label: 'Active' },
  { value: 'completed', label: 'Completed' },
  { value: 'failed', label: 'Failed' },
  { value: 'delayed', label: 'Delayed' },
]

export const QueueDetail = memo(() => {
  const queryClient = useQueryClient()
  const selectedQueue = useBullMQStore((state) => state.selectedQueue)
  const selectedStatus = useBullMQStore((state) => state.selectedStatus)
  const setSelectedStatus = useBullMQStore((state) => state.setSelectedStatus)
  const { pauseQueue, resumeQueue, cleanQueue, drainQueue } = useQueues()

  const handlePause = useCallback(async () => {
    if (!selectedQueue) return
    await pauseQueue(selectedQueue.name)
  }, [selectedQueue, pauseQueue])

  const handleResume = useCallback(async () => {
    if (!selectedQueue) return
    await resumeQueue(selectedQueue.name)
  }, [selectedQueue, resumeQueue])

  const handleCleanCompleted = useCallback(async () => {
    if (!selectedQueue) return
    await cleanQueue(selectedQueue.name, 'completed', 0, 1000)
  }, [selectedQueue, cleanQueue])

  const handleCleanFailed = useCallback(async () => {
    if (!selectedQueue) return
    await cleanQueue(selectedQueue.name, 'failed', 0, 1000)
  }, [selectedQueue, cleanQueue])

  const handleDrain = useCallback(async () => {
    if (!selectedQueue) return
    await drainQueue(selectedQueue.name)
  }, [selectedQueue, drainQueue])

  const handleRefresh = useCallback(() => {
    if (!selectedQueue) return
    queryClient.invalidateQueries({ queryKey: ['jobs', selectedQueue.name] })
  }, [selectedQueue, queryClient])

  if (!selectedQueue) {
    return (
      <div className="flex items-center justify-center h-full text-muted-foreground">
        Select a queue from the list to view details
      </div>
    )
  }

  return (
    <TooltipProvider>
      <div className="flex flex-col h-full">
        <div className="flex items-center justify-between p-3 border-b border-border">
          <div className="flex items-center gap-3">
            <h2 className="font-semibold text-lg">{selectedQueue.displayName}</h2>
            {selectedQueue.isPaused && (
              <span className="px-2 py-0.5 text-xs rounded bg-yellow-500/20 text-yellow-600">Paused</span>
            )}
          </div>
          <div className="flex items-center gap-2">
            <Tooltip>
              <TooltipTrigger asChild>
                <Button variant="ghost" size="icon" onClick={handleRefresh}>
                  <RefreshCw className="h-4 w-4" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>
                <p>Refresh queue stats and jobs list</p>
              </TooltipContent>
            </Tooltip>
            {selectedQueue.isPaused ? (
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button variant="ghost" size="sm" onClick={handleResume}>
                    <Play className="mr-2 h-4 w-4" />
                    Resume
                  </Button>
                </TooltipTrigger>
                <TooltipContent>
                  <p>Resume processing jobs in this queue</p>
                </TooltipContent>
              </Tooltip>
            ) : (
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button variant="ghost" size="sm" onClick={handlePause}>
                    <Pause className="mr-2 h-4 w-4" />
                    Pause
                  </Button>
                </TooltipTrigger>
                <TooltipContent>
                  <p>Stop workers from picking up new jobs</p>
                </TooltipContent>
              </Tooltip>
            )}
            <Tooltip>
              <TooltipTrigger asChild>
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button variant="ghost" size="icon">
                      <MoreVertical className="h-4 w-4" />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end" className="bg-background text-foreground">
                    <DropdownMenuItem onClick={handleCleanCompleted}>
                      <Trash className="mr-2 h-4 w-4" />
                      Clean Completed
                    </DropdownMenuItem>
                    <DropdownMenuItem onClick={handleCleanFailed}>
                      <Trash className="mr-2 h-4 w-4" />
                      Clean Failed
                    </DropdownMenuItem>
                    <DropdownMenuSeparator />
                    <DropdownMenuItem onClick={handleDrain} className="text-destructive">
                      <Trash className="mr-2 h-4 w-4" />
                      Drain Queue
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              </TooltipTrigger>
              <TooltipContent>
                <p>More actions</p>
              </TooltipContent>
            </Tooltip>
          </div>
        </div>

        <div className="p-3 border-b border-border">
          <div className="grid grid-cols-6 gap-4 text-center">
            <div>
              <div className="text-2xl font-bold text-blue-500">{selectedQueue.stats.waiting}</div>
              <div className="text-xs text-muted-foreground">Waiting</div>
            </div>
            <div>
              <div className="text-2xl font-bold text-yellow-500">{selectedQueue.stats.active}</div>
              <div className="text-xs text-muted-foreground">Active</div>
            </div>
            <div>
              <div className="text-2xl font-bold text-green-500">{selectedQueue.stats.completed}</div>
              <div className="text-xs text-muted-foreground">Completed</div>
            </div>
            <div>
              <div className="text-2xl font-bold text-destructive">{selectedQueue.stats.failed}</div>
              <div className="text-xs text-muted-foreground">Failed</div>
            </div>
            <div>
              <div className="text-2xl font-bold text-purple-500">{selectedQueue.stats.delayed}</div>
              <div className="text-xs text-muted-foreground">Delayed</div>
            </div>
            <div>
              <div className="text-2xl font-bold text-muted-foreground">{selectedQueue.stats.paused}</div>
              <div className="text-xs text-muted-foreground">Paused</div>
            </div>
          </div>
        </div>

        <Tabs
          value={selectedStatus}
          onValueChange={(v) => setSelectedStatus(v as JobStatus)}
          className="flex-1 flex flex-col"
        >
          <div className="px-3 pt-2 border-b border-border">
            <TabsList>
              {STATUS_TABS.map((tab) => (
                <TabsTrigger
                  key={tab.value}
                  value={tab.value}
                  className={cn(
                    'relative',
                    selectedStatus === tab.value &&
                      'after:absolute after:bottom-0 after:left-0 after:right-0 after:h-0.5 after:bg-primary',
                  )}
                >
                  {tab.label}
                  {selectedQueue.stats[tab.value] > 0 && (
                    <span className="ml-1.5 px-1.5 py-0.5 text-[10px] rounded-full bg-muted">
                      {selectedQueue.stats[tab.value]}
                    </span>
                  )}
                </TabsTrigger>
              ))}
            </TabsList>
          </div>
          <div className="flex-1 overflow-auto">
            <JobsTable />
          </div>
        </Tabs>
      </div>
    </TooltipProvider>
  )
})
QueueDetail.displayName = 'QueueDetail'
