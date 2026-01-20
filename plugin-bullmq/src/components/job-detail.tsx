import { Button, Sidebar } from '@motiadev/ui'
import { formatDistanceToNow } from 'date-fns'
import { ArrowUpRight, RefreshCw, Trash, X } from 'lucide-react'
import { memo, useCallback } from 'react'
import JsonView from 'react18-json-view'
import 'react18-json-view/src/style.css'
import { usePromoteJob, useRemoveJob, useRetryJob } from '../hooks/use-jobs-mutations'
import { useBullMQStore } from '../stores/use-bullmq-store'

const JobDataTab = memo<{ data: unknown }>(({ data }) => (
  <div className="bg-muted/30 p-4 rounded">
    <JsonView src={data as object} theme="atom" collapsed={2} />
  </div>
))
JobDataTab.displayName = 'JobDataTab'

const JobOptionsTab = memo<{ opts: Record<string, unknown> }>(({ opts }) => (
  <div className="bg-muted/30 p-4 rounded">
    <JsonView src={opts} theme="atom" collapsed={2} />
  </div>
))
JobOptionsTab.displayName = 'JobOptionsTab'

const JobResultTab = memo<{ returnvalue: unknown }>(({ returnvalue }) => (
  <div className="bg-muted/30 p-4 rounded">
    <JsonView src={returnvalue as object} theme="atom" collapsed={2} />
  </div>
))
JobResultTab.displayName = 'JobResultTab'

const JobErrorTab = memo<{ failedReason?: string; stacktrace?: string[] }>(({ failedReason, stacktrace }) => (
  <div className="space-y-4">
    <div>
      <div className="text-sm font-semibold text-destructive mb-1">Error Message</div>
      <div className="font-mono text-sm bg-destructive/10 p-3 rounded text-destructive">{failedReason}</div>
    </div>
    {stacktrace && stacktrace.length > 0 && (
      <div>
        <div className="text-sm font-semibold text-muted-foreground mb-1">Stack Trace</div>
        <pre className="font-mono text-xs bg-muted p-3 rounded overflow-auto max-h-[300px]">
          {stacktrace.join('\n')}
        </pre>
      </div>
    )}
  </div>
))
JobErrorTab.displayName = 'JobErrorTab'

export const JobDetail = memo(() => {
  const selectedJob = useBullMQStore((state) => state.selectedJob)
  const selectedQueue = useBullMQStore((state) => state.selectedQueue)
  const jobDetailOpen = useBullMQStore((state) => state.jobDetailOpen)
  const setJobDetailOpen = useBullMQStore((state) => state.setJobDetailOpen)
  const setSelectedJob = useBullMQStore((state) => state.setSelectedJob)
  const retryJobMutation = useRetryJob()
  const removeJobMutation = useRemoveJob()
  const promoteJobMutation = usePromoteJob()

  const handleClose = useCallback(() => {
    setJobDetailOpen(false)
    setSelectedJob(null)
  }, [setJobDetailOpen, setSelectedJob])

  const handleRetry = useCallback(() => {
    if (!selectedQueue || !selectedJob) return
    retryJobMutation.mutate({ queueName: selectedQueue.name, jobId: selectedJob.id })
    handleClose()
  }, [selectedQueue, selectedJob, retryJobMutation, handleClose])

  const handleRemove = useCallback(() => {
    if (!selectedQueue || !selectedJob) return
    removeJobMutation.mutate({ queueName: selectedQueue.name, jobId: selectedJob.id })
    handleClose()
  }, [selectedQueue, selectedJob, removeJobMutation, handleClose])

  const handlePromote = useCallback(() => {
    if (!selectedQueue || !selectedJob) return
    promoteJobMutation.mutate({ queueName: selectedQueue.name, jobId: selectedJob.id })
    handleClose()
  }, [selectedQueue, selectedJob, promoteJobMutation, handleClose])

  if (!jobDetailOpen || !selectedJob || !selectedQueue) return null

  const hasError = !!selectedJob.failedReason
  const isDelayed = selectedJob.delay && selectedJob.delay > 0
  const hasReturnValue = selectedJob.returnvalue !== undefined && selectedJob.returnvalue !== null

  const tabs = [
    { label: 'Data', content: <JobDataTab data={selectedJob.data} /> },
    { label: 'Options', content: <JobOptionsTab opts={selectedJob.opts} /> },
    ...(hasReturnValue ? [{ label: 'Result', content: <JobResultTab returnvalue={selectedJob.returnvalue} /> }] : []),
    ...(hasError
      ? [
          {
            label: 'Error',
            content: <JobErrorTab failedReason={selectedJob.failedReason} stacktrace={selectedJob.stacktrace} />,
          },
        ]
      : []),
  ]

  const statusElement = hasError ? (
    <span className="text-destructive">Failed</span>
  ) : selectedJob.finishedOn ? (
    <span className="text-green-500">Completed</span>
  ) : selectedJob.processedOn ? (
    <span className="text-yellow-500">Processing</span>
  ) : isDelayed ? (
    <span className="text-purple-500">Delayed</span>
  ) : (
    <span className="text-blue-500">Waiting</span>
  )

  return (
    <Sidebar
      onClose={handleClose}
      title={selectedJob.name}
      initialWidth={600}
      tabs={tabs}
      actions={[{ icon: <X />, onClick: handleClose, label: 'Close' }]}
    >
      <div className="space-y-4">
        <div className="flex gap-2">
          {hasError && (
            <Button variant="outline" size="sm" onClick={handleRetry}>
              <RefreshCw className="mr-2 h-4 w-4" />
              Retry
            </Button>
          )}
          {isDelayed && (
            <Button variant="outline" size="sm" onClick={handlePromote}>
              <ArrowUpRight className="mr-2 h-4 w-4" />
              Promote
            </Button>
          )}
          <Button variant="outline" size="sm" onClick={handleRemove} className="text-destructive">
            <Trash className="mr-2 h-4 w-4" />
            Remove
          </Button>
        </div>

        <div className="grid grid-cols-2 gap-4 text-sm">
          <div>
            <span className="text-muted-foreground">Job ID</span>
            <div className="font-mono text-xs">{selectedJob.id}</div>
          </div>
          <div>
            <span className="text-muted-foreground">Status</span>
            <div className="font-semibold">{statusElement}</div>
          </div>
          <div>
            <span className="text-muted-foreground">Created</span>
            <div>{formatDistanceToNow(selectedJob.timestamp, { addSuffix: true })}</div>
          </div>
          <div>
            <span className="text-muted-foreground">Attempts</span>
            <div className="font-semibold">{selectedJob.attemptsMade}</div>
          </div>
          <div>
            <span className="text-muted-foreground">Progress</span>
            <div className="font-semibold">
              {typeof selectedJob.progress === 'number' ? `${selectedJob.progress}%` : '-'}
            </div>
          </div>
          <div>
            <span className="text-muted-foreground">Delay</span>
            <div className="font-semibold">{selectedJob.delay ? `${selectedJob.delay}ms` : '-'}</div>
          </div>
        </div>
      </div>
    </Sidebar>
  )
})
JobDetail.displayName = 'JobDetail'
