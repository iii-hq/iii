import type { ApiRequest, ApiResponse, MotiaPluginContext } from '@motiadev/core'
import type { Redis } from 'ioredis'
import { discoverQueueNames, getOrCreateQueue, getQueueInfo } from './streams/queues-stream'
import type { CleanOptions, JobInfo, JobStatus, QueueInfo } from './types/queue'

const discoverQueues = async (connection: Redis, prefix: string, dlqSuffix: string): Promise<QueueInfo[]> => {
  const queueNames = await discoverQueueNames(connection, prefix)
  const queueInfos: QueueInfo[] = []

  for (const name of queueNames) {
    const info = await getQueueInfo(name, connection, prefix, dlqSuffix)
    queueInfos.push(info)
  }

  return queueInfos
}

export const api = (
  { registerApi }: MotiaPluginContext,
  prefix: string,
  dlqSuffix: string,
  connection: Redis,
): void => {
  registerApi({ method: 'GET', path: '/__motia/bullmq/queues' }, async (): Promise<ApiResponse> => {
    try {
      const queues = await discoverQueues(connection, prefix, dlqSuffix)
      return { status: 200, body: { queues } }
    } catch (error) {
      return { status: 500, body: { error: error instanceof Error ? error.message : 'Unknown error' } }
    }
  })

  registerApi(
    { method: 'GET', path: '/__motia/bullmq/queues/:name' },
    async (req: ApiRequest): Promise<ApiResponse> => {
      try {
        const name = req.pathParams.name as string
        const queue = getOrCreateQueue(name, connection, prefix)

        const [isPaused, counts] = await Promise.all([queue.isPaused(), queue.getJobCounts()])

        return {
          status: 200,
          body: {
            name,
            displayName: name,
            isPaused,
            isDLQ: name.endsWith(dlqSuffix),
            stats: {
              waiting: counts.waiting || 0,
              active: counts.active || 0,
              completed: counts.completed || 0,
              failed: counts.failed || 0,
              delayed: counts.delayed || 0,
              paused: counts.paused || 0,
              prioritized: counts.prioritized || 0,
            },
          },
        }
      } catch (error) {
        return { status: 500, body: { error: error instanceof Error ? error.message : 'Unknown error' } }
      }
    },
  )

  registerApi(
    { method: 'POST', path: '/__motia/bullmq/queues/:name/pause' },
    async (req: ApiRequest): Promise<ApiResponse> => {
      try {
        const name = req.pathParams.name as string
        const queue = getOrCreateQueue(name, connection, prefix)
        await queue.pause()
        return { status: 200, body: { message: 'Queue paused' } }
      } catch (error) {
        return { status: 500, body: { error: error instanceof Error ? error.message : 'Unknown error' } }
      }
    },
  )

  registerApi(
    { method: 'POST', path: '/__motia/bullmq/queues/:name/resume' },
    async (req: ApiRequest): Promise<ApiResponse> => {
      try {
        const name = req.pathParams.name as string
        const queue = getOrCreateQueue(name, connection, prefix)
        await queue.resume()
        return { status: 200, body: { message: 'Queue resumed' } }
      } catch (error) {
        return { status: 500, body: { error: error instanceof Error ? error.message : 'Unknown error' } }
      }
    },
  )

  registerApi(
    { method: 'POST', path: '/__motia/bullmq/queues/:name/clean' },
    async (req: ApiRequest): Promise<ApiResponse> => {
      try {
        const name = req.pathParams.name as string
        const body = req.body as Partial<CleanOptions>
        const options: CleanOptions = {
          grace: body.grace ?? 0,
          limit: body.limit ?? 1000,
          status: body.status ?? 'completed',
        }
        const queue = getOrCreateQueue(name, connection, prefix)
        const deletedIds = await queue.clean(options.grace, options.limit, options.status)
        return { status: 200, body: { deleted: deletedIds.length, ids: deletedIds } }
      } catch (error) {
        return { status: 500, body: { error: error instanceof Error ? error.message : 'Unknown error' } }
      }
    },
  )

  registerApi(
    { method: 'POST', path: '/__motia/bullmq/queues/:name/drain' },
    async (req: ApiRequest): Promise<ApiResponse> => {
      try {
        const name = req.pathParams.name as string
        const queue = getOrCreateQueue(name, connection, prefix)
        await queue.drain()
        return { status: 200, body: { message: 'Queue drained' } }
      } catch (error) {
        return { status: 500, body: { error: error instanceof Error ? error.message : 'Unknown error' } }
      }
    },
  )

  registerApi(
    { method: 'GET', path: '/__motia/bullmq/queues/:name/jobs' },
    async (req: ApiRequest): Promise<ApiResponse> => {
      try {
        const name = req.pathParams.name as string
        const status = (req.queryParams.status as JobStatus) || 'waiting'
        const start = parseInt(req.queryParams.start as string, 10) || 0
        const end = parseInt(req.queryParams.end as string, 10) || 100

        const queue = getOrCreateQueue(name, connection, prefix)
        const rawJobs = await queue.getJobs([status], start, end)

        const jobs: JobInfo[] = rawJobs.map((job) => ({
          id: job.id || '',
          name: job.name,
          data: job.data,
          opts: job.opts as Record<string, unknown>,
          progress: typeof job.progress === 'object' ? JSON.stringify(job.progress) : job.progress,
          timestamp: job.timestamp,
          attemptsMade: job.attemptsMade,
          processedOn: job.processedOn,
          finishedOn: job.finishedOn,
          returnvalue: job.returnvalue,
          failedReason: job.failedReason,
          stacktrace: job.stacktrace,
        }))

        return { status: 200, body: { jobs } }
      } catch (error) {
        return { status: 500, body: { error: error instanceof Error ? error.message : 'Unknown error' } }
      }
    },
  )

  registerApi(
    { method: 'GET', path: '/__motia/bullmq/queues/:queueName/jobs/:jobId' },
    async (req: ApiRequest): Promise<ApiResponse> => {
      try {
        const queueName = req.pathParams.queueName as string
        const jobId = req.pathParams.jobId as string

        const queue = getOrCreateQueue(queueName, connection, prefix)
        const job = await queue.getJob(jobId)

        if (!job) {
          return { status: 404, body: { error: 'Job not found' } }
        }

        const jobInfo: JobInfo = {
          id: job.id || '',
          name: job.name,
          data: job.data,
          opts: job.opts as Record<string, unknown>,
          progress: typeof job.progress === 'object' ? JSON.stringify(job.progress) : job.progress,
          timestamp: job.timestamp,
          attemptsMade: job.attemptsMade,
          processedOn: job.processedOn,
          finishedOn: job.finishedOn,
          returnvalue: job.returnvalue,
          failedReason: job.failedReason,
          stacktrace: job.stacktrace,
        }

        return { status: 200, body: jobInfo }
      } catch (error) {
        return { status: 500, body: { error: error instanceof Error ? error.message : 'Unknown error' } }
      }
    },
  )

  registerApi(
    { method: 'POST', path: '/__motia/bullmq/queues/:queueName/jobs/:jobId/retry' },
    async (req: ApiRequest): Promise<ApiResponse> => {
      try {
        const queueName = req.pathParams.queueName as string
        const jobId = req.pathParams.jobId as string

        const queue = getOrCreateQueue(queueName, connection, prefix)
        const job = await queue.getJob(jobId)

        if (!job) {
          return { status: 404, body: { error: 'Job not found' } }
        }

        await job.retry()
        return { status: 200, body: { message: 'Job retried' } }
      } catch (error) {
        return { status: 500, body: { error: error instanceof Error ? error.message : 'Unknown error' } }
      }
    },
  )

  registerApi(
    { method: 'POST', path: '/__motia/bullmq/queues/:queueName/jobs/:jobId/remove' },
    async (req: ApiRequest): Promise<ApiResponse> => {
      try {
        const queueName = req.pathParams.queueName as string
        const jobId = req.pathParams.jobId as string

        const queue = getOrCreateQueue(queueName, connection, prefix)
        const job = await queue.getJob(jobId)

        if (!job) {
          return { status: 404, body: { error: 'Job not found' } }
        }

        await job.remove()
        return { status: 200, body: { message: 'Job removed' } }
      } catch (error) {
        return { status: 500, body: { error: error instanceof Error ? error.message : 'Unknown error' } }
      }
    },
  )

  registerApi(
    { method: 'POST', path: '/__motia/bullmq/queues/:queueName/jobs/:jobId/promote' },
    async (req: ApiRequest): Promise<ApiResponse> => {
      try {
        const queueName = req.pathParams.queueName as string
        const jobId = req.pathParams.jobId as string

        const queue = getOrCreateQueue(queueName, connection, prefix)
        const job = await queue.getJob(jobId)

        if (!job) {
          return { status: 404, body: { error: 'Job not found' } }
        }

        await job.promote()
        return { status: 200, body: { message: 'Job promoted' } }
      } catch (error) {
        return { status: 500, body: { error: error instanceof Error ? error.message : 'Unknown error' } }
      }
    },
  )

  registerApi(
    { method: 'GET', path: '/__motia/bullmq/dlq/:name/jobs' },
    async (req: ApiRequest): Promise<ApiResponse> => {
      try {
        const name = req.pathParams.name as string
        const start = parseInt(req.queryParams.start as string, 10) || 0
        const end = parseInt(req.queryParams.end as string, 10) || 100

        const dlqName = name.endsWith(dlqSuffix) ? name : `${name}${dlqSuffix}`
        const queue = getOrCreateQueue(dlqName, connection, prefix)
        const rawJobs = await queue.getJobs(['waiting', 'completed'], start, end)

        const jobs = rawJobs.map((job) => ({
          id: job.id || '',
          name: job.name,
          data: job.data,
          timestamp: job.timestamp,
          originalEvent: job.data?.originalEvent,
          failureReason: job.data?.failureReason || job.failedReason,
          failureTimestamp: job.data?.failureTimestamp || job.finishedOn,
          attemptsMade: job.data?.attemptsMade || job.attemptsMade,
        }))

        return { status: 200, body: { jobs } }
      } catch (error) {
        return { status: 500, body: { error: error instanceof Error ? error.message : 'Unknown error' } }
      }
    },
  )

  registerApi(
    { method: 'POST', path: '/__motia/bullmq/dlq/:name/retry/:jobId' },
    async (req: ApiRequest): Promise<ApiResponse> => {
      try {
        const name = req.pathParams.name as string
        const jobId = req.pathParams.jobId as string

        const dlqName = name.endsWith(dlqSuffix) ? name : `${name}${dlqSuffix}`
        const dlqQueue = getOrCreateQueue(dlqName, connection, prefix)
        const job = await dlqQueue.getJob(jobId)

        if (!job) {
          return { status: 404, body: { error: 'Job not found in DLQ' } }
        }

        const originalEvent = job.data?.originalEvent
        if (originalEvent) {
          const originalQueueName = dlqName.replace(dlqSuffix, '')
          const originalQueue = getOrCreateQueue(originalQueueName, connection, prefix)

          const jobData: Record<string, unknown> = {
            topic: originalEvent.topic,
            data: originalEvent.data,
            traceId: originalEvent.traceId,
          }

          if (originalEvent.flows) {
            jobData.flows = originalEvent.flows
          }
          if (originalEvent.messageGroupId) {
            jobData.messageGroupId = originalEvent.messageGroupId
          }

          await originalQueue.add(originalEvent.topic || job.name, jobData)
        }

        await job.remove()
        return { status: 200, body: { message: 'Job retried from DLQ' } }
      } catch (error) {
        return { status: 500, body: { error: error instanceof Error ? error.message : 'Unknown error' } }
      }
    },
  )

  registerApi(
    { method: 'POST', path: '/__motia/bullmq/dlq/:name/retry-all' },
    async (req: ApiRequest): Promise<ApiResponse> => {
      try {
        const name = req.pathParams.name as string

        const dlqName = name.endsWith(dlqSuffix) ? name : `${name}${dlqSuffix}`
        const dlqQueue = getOrCreateQueue(dlqName, connection, prefix)
        const jobs = await dlqQueue.getJobs(['waiting', 'completed'])

        const originalQueueName = dlqName.replace(dlqSuffix, '')
        const originalQueue = getOrCreateQueue(originalQueueName, connection, prefix)

        let count = 0
        for (const job of jobs) {
          const originalEvent = job.data?.originalEvent
          if (originalEvent) {
            const jobData: Record<string, unknown> = {
              topic: originalEvent.topic,
              data: originalEvent.data,
              traceId: originalEvent.traceId,
            }

            if (originalEvent.flows) {
              jobData.flows = originalEvent.flows
            }
            if (originalEvent.messageGroupId) {
              jobData.messageGroupId = originalEvent.messageGroupId
            }

            await originalQueue.add(originalEvent.topic || job.name, jobData)
          }
          await job.remove()
          count++
        }

        return { status: 200, body: { message: `Retried ${count} jobs from DLQ`, count } }
      } catch (error) {
        return { status: 500, body: { error: error instanceof Error ? error.message : 'Unknown error' } }
      }
    },
  )

  registerApi(
    { method: 'POST', path: '/__motia/bullmq/dlq/:name/clear' },
    async (req: ApiRequest): Promise<ApiResponse> => {
      try {
        const name = req.pathParams.name as string

        const dlqName = name.endsWith(dlqSuffix) ? name : `${name}${dlqSuffix}`
        const queue = getOrCreateQueue(dlqName, connection, prefix)
        await queue.obliterate({ force: true })

        return { status: 200, body: { message: 'DLQ cleared' } }
      } catch (error) {
        return { status: 500, body: { error: error instanceof Error ? error.message : 'Unknown error' } }
      }
    },
  )
}
