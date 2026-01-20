import {
  Button,
  Sidebar,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@motiadev/ui'
import { formatDistanceToNow } from 'date-fns'
import { RefreshCw, Trash, X } from 'lucide-react'
import { memo, useCallback, useState } from 'react'
import JsonView from 'react18-json-view'
import 'react18-json-view/src/style.css'
import { useClearDLQ, useRetryAllFromDLQ, useRetryFromDLQ } from '../hooks/use-jobs-mutations'
import { useDLQJobsQuery } from '../hooks/use-jobs-query'
import { useBullMQStore } from '../stores/use-bullmq-store'
import type { DLQJobInfo } from '../types/queue'

const DLQJobDetailContent = memo<{
  job: DLQJobInfo
  onRetry: () => void
}>(({ job, onRetry }) => (
  <div className="space-y-4">
    <div className="flex gap-2">
      <Button variant="outline" size="sm" onClick={onRetry}>
        <RefreshCw className="mr-2 h-4 w-4" />
        Retry
      </Button>
    </div>

    <div className="grid grid-cols-2 gap-4 text-sm">
      <div>
        <span className="text-muted-foreground">Job ID</span>
        <div className="font-mono text-xs">{job.id}</div>
      </div>
      <div>
        <span className="text-muted-foreground">Original Job ID</span>
        <div className="font-mono text-xs">{job.originalJobId || '-'}</div>
      </div>
      <div>
        <span className="text-muted-foreground">Attempts Made</span>
        <div className="font-semibold">{job.attemptsMade}</div>
      </div>
      <div>
        <span className="text-muted-foreground">Failed At</span>
        <div>{formatDistanceToNow(job.failureTimestamp, { addSuffix: true })}</div>
      </div>
    </div>

    <div>
      <div className="text-sm font-semibold text-destructive mb-2">Failure Reason</div>
      <div className="font-mono text-sm bg-destructive/10 p-3 rounded text-destructive">{job.failureReason}</div>
    </div>
  </div>
))
DLQJobDetailContent.displayName = 'DLQJobDetailContent'

const DLQJobDataTab = memo<{ data: unknown }>(({ data }) => (
  <div className="bg-muted/30 p-4 rounded overflow-auto max-h-[400px]">
    <JsonView src={data as object} theme="atom" collapsed={2} />
  </div>
))
DLQJobDataTab.displayName = 'DLQJobDataTab'

export const DLQPanel = memo(() => {
  const selectedQueue = useBullMQStore((state) => state.selectedQueue)
  const {
    data: dlqJobs = [],
    isLoading,
    refetch,
  } = useDLQJobsQuery(selectedQueue?.isDLQ ? selectedQueue.name : undefined)
  const retryFromDLQMutation = useRetryFromDLQ()
  const retryAllFromDLQMutation = useRetryAllFromDLQ()
  const clearDLQMutation = useClearDLQ()
  const [selectedDlqJob, setSelectedDlqJob] = useState<DLQJobInfo | null>(null)

  const handleRetry = useCallback(
    (jobId: string) => {
      if (!selectedQueue) return
      retryFromDLQMutation.mutate({ queueName: selectedQueue.name, jobId })
    },
    [selectedQueue, retryFromDLQMutation],
  )

  const handleRetryAll = useCallback(() => {
    if (!selectedQueue) return
    retryAllFromDLQMutation.mutate({ queueName: selectedQueue.name })
  }, [selectedQueue, retryAllFromDLQMutation])

  const handleClear = useCallback(() => {
    if (!selectedQueue) return
    clearDLQMutation.mutate({ queueName: selectedQueue.name })
  }, [selectedQueue, clearDLQMutation])

  const handleCloseSidePanel = useCallback(() => {
    setSelectedDlqJob(null)
  }, [])

  const handleRetryAndClose = useCallback(() => {
    if (selectedDlqJob) {
      handleRetry(selectedDlqJob.id)
      setSelectedDlqJob(null)
    }
  }, [selectedDlqJob, handleRetry])

  if (!selectedQueue?.isDLQ) return null

  return (
    <TooltipProvider>
      <div className="flex flex-col h-full">
        <div className="flex items-center justify-between p-3 border-b border-border">
          <div>
            <h2 className="font-semibold text-lg">{selectedQueue.displayName}</h2>
            <p className="text-sm text-muted-foreground">
              {dlqJobs.length} failed job{dlqJobs.length !== 1 ? 's' : ''} in dead letter queue
            </p>
          </div>
          <div className="flex items-center gap-2">
            <Tooltip>
              <TooltipTrigger asChild>
                <Button variant="ghost" size="icon" onClick={() => refetch()} disabled={isLoading}>
                  <RefreshCw className={`h-4 w-4 ${isLoading ? 'animate-spin' : ''}`} />
                </Button>
              </TooltipTrigger>
              <TooltipContent>
                <p>Refresh DLQ jobs list</p>
              </TooltipContent>
            </Tooltip>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button variant="outline" size="sm" onClick={handleRetryAll} disabled={dlqJobs.length === 0}>
                  <RefreshCw className="mr-2 h-4 w-4" />
                  Retry All
                </Button>
              </TooltipTrigger>
              <TooltipContent>
                <p>Re-queue all failed jobs to the original queue</p>
              </TooltipContent>
            </Tooltip>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={handleClear}
                  disabled={dlqJobs.length === 0}
                  className="text-destructive"
                >
                  <Trash className="mr-2 h-4 w-4" />
                  Clear All
                </Button>
              </TooltipTrigger>
              <TooltipContent>
                <p>Permanently delete all jobs from DLQ</p>
              </TooltipContent>
            </Tooltip>
          </div>
        </div>

        {dlqJobs.length === 0 ? (
          <div className="flex items-center justify-center flex-1 text-muted-foreground">
            No jobs in dead letter queue
          </div>
        ) : (
          <div className="flex-1 overflow-auto">
            <Table>
              <TableHeader className="sticky top-0 bg-background/95 backdrop-blur-sm">
                <TableRow>
                  <TableHead className="w-[180px]">Job ID</TableHead>
                  <TableHead className="w-[180px]">Original Job</TableHead>
                  <TableHead>Failure Reason</TableHead>
                  <TableHead className="w-[100px]">Attempts</TableHead>
                  <TableHead className="w-[150px]">Failed At</TableHead>
                  <TableHead className="w-[100px]">Actions</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {dlqJobs.map((job) => (
                  <TableRow
                    key={job.id}
                    className="cursor-pointer hover:bg-muted-foreground/10"
                    onClick={() => setSelectedDlqJob(job)}
                  >
                    <TableCell className="font-mono text-xs">{job.id}</TableCell>
                    <TableCell className="font-mono text-xs text-muted-foreground">
                      {job.originalJobId || '-'}
                    </TableCell>
                    <TableCell className="text-destructive text-sm truncate max-w-[300px]">
                      {job.failureReason}
                    </TableCell>
                    <TableCell>{job.attemptsMade}</TableCell>
                    <TableCell className="text-xs text-muted-foreground">
                      {formatDistanceToNow(job.failureTimestamp, { addSuffix: true })}
                    </TableCell>
                    <TableCell onClick={(e) => e.stopPropagation()}>
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <Button variant="ghost" size="sm" onClick={() => handleRetry(job.id)}>
                            <RefreshCw className="h-4 w-4" />
                          </Button>
                        </TooltipTrigger>
                        <TooltipContent>
                          <p>Retry this job</p>
                        </TooltipContent>
                      </Tooltip>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
        )}

        {selectedDlqJob && (
          <Sidebar
            onClose={handleCloseSidePanel}
            title="Dead Letter Job"
            initialWidth={600}
            tabs={[
              {
                label: 'Details',
                content: <DLQJobDetailContent job={selectedDlqJob} onRetry={handleRetryAndClose} />,
              },
              {
                label: 'Event Data',
                content: <DLQJobDataTab data={selectedDlqJob.originalEvent} />,
              },
            ]}
            actions={[{ icon: <X />, onClick: handleCloseSidePanel, label: 'Close' }]}
          />
        )}
      </div>
    </TooltipProvider>
  )
})
DLQPanel.displayName = 'DLQPanel'
