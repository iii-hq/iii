import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useBullMQStore } from '../stores/use-bullmq-store'

const retryJobFn = async ({ queueName, jobId }: { queueName: string; jobId: string }) => {
  const response = await fetch(
    `/__motia/bullmq/queues/${encodeURIComponent(queueName)}/jobs/${encodeURIComponent(jobId)}/retry`,
    { method: 'POST' },
  )
  if (!response.ok) {
    throw new Error('Failed to retry job')
  }
}

const removeJobFn = async ({ queueName, jobId }: { queueName: string; jobId: string }) => {
  const response = await fetch(
    `/__motia/bullmq/queues/${encodeURIComponent(queueName)}/jobs/${encodeURIComponent(jobId)}/remove`,
    { method: 'POST' },
  )
  if (!response.ok) {
    throw new Error('Failed to remove job')
  }
}

const promoteJobFn = async ({ queueName, jobId }: { queueName: string; jobId: string }) => {
  const response = await fetch(
    `/__motia/bullmq/queues/${encodeURIComponent(queueName)}/jobs/${encodeURIComponent(jobId)}/promote`,
    { method: 'POST' },
  )
  if (!response.ok) {
    throw new Error('Failed to promote job')
  }
}

const retryFromDLQFn = async ({ queueName, jobId }: { queueName: string; jobId: string }) => {
  const response = await fetch(
    `/__motia/bullmq/dlq/${encodeURIComponent(queueName)}/retry/${encodeURIComponent(jobId)}`,
    { method: 'POST' },
  )
  if (!response.ok) {
    throw new Error('Failed to retry from DLQ')
  }
}

const retryAllFromDLQFn = async ({ queueName }: { queueName: string }) => {
  const response = await fetch(`/__motia/bullmq/dlq/${encodeURIComponent(queueName)}/retry-all`, { method: 'POST' })
  if (!response.ok) {
    throw new Error('Failed to retry all from DLQ')
  }
}

const clearDLQFn = async ({ queueName }: { queueName: string }) => {
  const response = await fetch(`/__motia/bullmq/dlq/${encodeURIComponent(queueName)}/clear`, { method: 'POST' })
  if (!response.ok) {
    throw new Error('Failed to clear DLQ')
  }
}

export const useRetryJob = () => {
  const queryClient = useQueryClient()
  const setError = useBullMQStore((state) => state.setError)

  return useMutation({
    mutationFn: retryJobFn,
    onSuccess: (_, { queueName }) => {
      queryClient.invalidateQueries({ queryKey: ['jobs', queueName] })
    },
    onError: (error) => {
      setError(error instanceof Error ? error.message : 'Failed to retry job')
    },
  })
}

export const useRemoveJob = () => {
  const queryClient = useQueryClient()
  const setError = useBullMQStore((state) => state.setError)

  return useMutation({
    mutationFn: removeJobFn,
    onSuccess: (_, { queueName }) => {
      queryClient.invalidateQueries({ queryKey: ['jobs', queueName] })
    },
    onError: (error) => {
      setError(error instanceof Error ? error.message : 'Failed to remove job')
    },
  })
}

export const usePromoteJob = () => {
  const queryClient = useQueryClient()
  const setError = useBullMQStore((state) => state.setError)

  return useMutation({
    mutationFn: promoteJobFn,
    onSuccess: (_, { queueName }) => {
      queryClient.invalidateQueries({ queryKey: ['jobs', queueName] })
    },
    onError: (error) => {
      setError(error instanceof Error ? error.message : 'Failed to promote job')
    },
  })
}

export const useRetryFromDLQ = () => {
  const queryClient = useQueryClient()
  const setError = useBullMQStore((state) => state.setError)

  return useMutation({
    mutationFn: retryFromDLQFn,
    onSuccess: (_, { queueName }) => {
      queryClient.invalidateQueries({ queryKey: ['dlq-jobs', queueName] })
    },
    onError: (error) => {
      setError(error instanceof Error ? error.message : 'Failed to retry from DLQ')
    },
  })
}

export const useRetryAllFromDLQ = () => {
  const queryClient = useQueryClient()
  const setError = useBullMQStore((state) => state.setError)

  return useMutation({
    mutationFn: retryAllFromDLQFn,
    onSuccess: (_, { queueName }) => {
      queryClient.invalidateQueries({ queryKey: ['dlq-jobs', queueName] })
    },
    onError: (error) => {
      setError(error instanceof Error ? error.message : 'Failed to retry all from DLQ')
    },
  })
}

export const useClearDLQ = () => {
  const queryClient = useQueryClient()
  const setError = useBullMQStore((state) => state.setError)

  return useMutation({
    mutationFn: clearDLQFn,
    onSuccess: (_, { queueName }) => {
      queryClient.invalidateQueries({ queryKey: ['dlq-jobs', queueName] })
    },
    onError: (error) => {
      setError(error instanceof Error ? error.message : 'Failed to clear DLQ')
    },
  })
}
