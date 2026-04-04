import { getDevtoolsApi } from '../config'
import { unwrapResponse } from '../utils'

export interface QueueTopic {
  name: string
  broker_type: string
  subscriber_count: number
}

export interface QueueStats {
  depth: number
  consumer_count: number
  dlq_depth: number
  config: Record<string, unknown> | null
}

export interface QueueDetail {
  topic: string
  stats: QueueStats
}

export interface DlqTopic {
  topic: string
  broker_type: string
  message_count: number
}

export interface DlqMessage {
  id: string
  payload: unknown
  error: string
  failed_at: number
  retries: number
  size_bytes: number
}

export async function fetchQueues(): Promise<{ queues: QueueTopic[] }> {
  const res = await fetch(`${getDevtoolsApi()}/queues`)
  return unwrapResponse(res)
}

export async function fetchQueueDetail(topic: string): Promise<QueueDetail> {
  const res = await fetch(`${getDevtoolsApi()}/queues/${encodeURIComponent(topic)}`)
  return unwrapResponse(res)
}

export async function publishToQueue(topic: string, data: unknown): Promise<void> {
  const res = await fetch(`${getDevtoolsApi()}/queues/${encodeURIComponent(topic)}/publish`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ data }),
  })
  return unwrapResponse(res)
}

export async function fetchDlqTopics(): Promise<{ topics: DlqTopic[] }> {
  const res = await fetch(`${getDevtoolsApi()}/dlq`)
  return unwrapResponse(res)
}

export async function fetchDlqMessages(
  topic: string,
  offset = 0,
  limit = 50,
): Promise<{ topic: string; messages: DlqMessage[] }> {
  const res = await fetch(`${getDevtoolsApi()}/dlq/${encodeURIComponent(topic)}/messages`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ offset, limit }),
  })
  return unwrapResponse(res)
}

export async function redriveDlq(topic: string): Promise<{ queue: string; redriven: number }> {
  const res = await fetch(`${getDevtoolsApi()}/dlq/${encodeURIComponent(topic)}/redrive`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({}),
  })
  return unwrapResponse(res)
}

export async function discardMessage(
  topic: string,
  messageId: string,
): Promise<{ queue: string; redriven: number }> {
  const res = await fetch(
    `${getDevtoolsApi()}/dlq/${encodeURIComponent(topic)}/messages/${encodeURIComponent(messageId)}/discard`,
    { method: 'DELETE' },
  )
  return unwrapResponse(res)
}

export async function redriveMessage(
  topic: string,
  messageId: string,
): Promise<{ queue: string; redriven: number }> {
  const res = await fetch(
    `${getDevtoolsApi()}/dlq/${encodeURIComponent(topic)}/messages/${encodeURIComponent(messageId)}/redrive`,
    {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({}),
    },
  )
  return unwrapResponse(res)
}
