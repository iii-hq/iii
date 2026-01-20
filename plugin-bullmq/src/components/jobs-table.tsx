import {
  Button,
  cn,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@motiadev/ui'
import { formatDistanceToNow } from 'date-fns'
import { ArrowUpRight, MoreVertical, RefreshCw, Trash } from 'lucide-react'
import { memo, useCallback } from 'react'
import { usePromoteJob, useRemoveJob, useRetryJob } from '../hooks/use-jobs-mutations'
import { useJobsQuery } from '../hooks/use-jobs-query'
import { useBullMQStore } from '../stores/use-bullmq-store'
import type { JobInfo } from '../types/queue'

type JobRowProps = {
  job: JobInfo
  queueName: string
  onSelect: () => void
  isSelected: boolean
}

const JobRow = memo<JobRowProps>(({ job, queueName, onSelect, isSelected }) => {
  const retryJobMutation = useRetryJob()
  const removeJobMutation = useRemoveJob()
  const promoteJobMutation = usePromoteJob()

  const handleRetry = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation()
      retryJobMutation.mutate({ queueName, jobId: job.id })
    },
    [queueName, job.id, retryJobMutation],
  )

  const handleRemove = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation()
      removeJobMutation.mutate({ queueName, jobId: job.id })
    },
    [queueName, job.id, removeJobMutation],
  )

  const handlePromote = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation()
      promoteJobMutation.mutate({ queueName, jobId: job.id })
    },
    [queueName, job.id, promoteJobMutation],
  )

  return (
    <TableRow
      onClick={onSelect}
      className={cn(
        'cursor-pointer border-0',
        isSelected ? 'bg-muted-foreground/10 hover:bg-muted-foreground/20' : 'hover:bg-muted-foreground/10',
      )}
    >
      <TableCell className="font-mono text-xs">{job.id}</TableCell>
      <TableCell className="font-medium">{job.name}</TableCell>
      <TableCell className="text-xs text-muted-foreground">
        {formatDistanceToNow(job.timestamp, { addSuffix: true })}
      </TableCell>
      <TableCell>
        <span className="text-xs">{job.attemptsMade}</span>
      </TableCell>
      <TableCell>
        {typeof job.progress === 'number' ? (
          <div className="flex items-center gap-2">
            <div className="w-16 h-1.5 bg-muted rounded-full overflow-hidden">
              <div className="h-full bg-primary transition-all" style={{ width: `${Math.min(100, job.progress)}%` }} />
            </div>
            <span className="text-xs text-muted-foreground">{job.progress}%</span>
          </div>
        ) : (
          <span className="text-xs text-muted-foreground">-</span>
        )}
      </TableCell>
      <TableCell onClick={(e) => e.stopPropagation()}>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size="icon" className="h-8 w-8">
              <MoreVertical className="h-4 w-4" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="bg-background text-foreground">
            {job.failedReason && (
              <DropdownMenuItem onClick={handleRetry}>
                <RefreshCw className="mr-2 h-4 w-4" />
                Retry
              </DropdownMenuItem>
            )}
            {job.delay && job.delay > 0 && (
              <DropdownMenuItem onClick={handlePromote}>
                <ArrowUpRight className="mr-2 h-4 w-4" />
                Promote
              </DropdownMenuItem>
            )}
            <DropdownMenuItem onClick={handleRemove} className="text-destructive">
              <Trash className="mr-2 h-4 w-4" />
              Remove
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </TableCell>
    </TableRow>
  )
})
JobRow.displayName = 'JobRow'

export const JobsTable = memo(() => {
  const { data: jobs = [], isLoading } = useJobsQuery()
  const selectedQueue = useBullMQStore((state) => state.selectedQueue)
  const selectedJob = useBullMQStore((state) => state.selectedJob)
  const setSelectedJob = useBullMQStore((state) => state.setSelectedJob)
  const setJobDetailOpen = useBullMQStore((state) => state.setJobDetailOpen)

  const handleSelectJob = useCallback(
    (job: JobInfo) => {
      setSelectedJob(job)
      setJobDetailOpen(true)
    },
    [setSelectedJob, setJobDetailOpen],
  )

  if (!selectedQueue) {
    return (
      <div className="flex items-center justify-center h-full text-muted-foreground">Select a queue to view jobs</div>
    )
  }

  if (isLoading) {
    return <div className="flex items-center justify-center h-full text-muted-foreground">Loading jobs...</div>
  }

  if (jobs.length === 0) {
    return <div className="flex items-center justify-center h-full text-muted-foreground">No jobs in this status</div>
  }

  return (
    <Table>
      <TableHeader className="sticky top-0 bg-background/95 backdrop-blur-sm">
        <TableRow>
          <TableHead className="w-[200px]">Job ID</TableHead>
          <TableHead>Name</TableHead>
          <TableHead className="w-[150px]">Created</TableHead>
          <TableHead className="w-[80px]">Attempts</TableHead>
          <TableHead className="w-[140px]">Progress</TableHead>
          <TableHead className="w-[60px]">Actions</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {jobs.map((job) => (
          <JobRow
            key={job.id}
            job={job}
            queueName={selectedQueue.name}
            onSelect={() => handleSelectJob(job)}
            isSelected={selectedJob?.id === job.id}
          />
        ))}
      </TableBody>
    </Table>
  )
})
JobsTable.displayName = 'JobsTable'
