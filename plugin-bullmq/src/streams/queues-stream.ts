import { StreamAdapter } from '@motiadev/core'
import { Queue, QueueEvents } from 'bullmq'
import type { Redis } from 'ioredis'
import type { QueueInfo } from '../types/queue'

const queues: Map<string, Queue> = new Map()
const queueEvents: Map<string, QueueEvents> = new Map()

export const getOrCreateQueue = (name: string, connection: Redis, prefix: string): Queue => {
  const existing = queues.get(name)
  if (existing) {
    return existing
  }
  const queue = new Queue(name, { connection, prefix })
  queues.set(name, queue)
  return queue
}

export const discoverQueueNames = async (connection: Redis, prefix: string): Promise<Set<string>> => {
  const pattern = `${prefix}:*:id`
  const keys = await connection.keys(pattern)
  const queueNames = new Set<string>()

  for (const key of keys) {
    const withoutPrefix = key.slice(prefix.length + 1)
    const withoutId = withoutPrefix.slice(0, -3)
    queueNames.add(withoutId)
  }

  return queueNames
}

export const getQueueInfo = async (
  name: string,
  connection: Redis,
  prefix: string,
  dlqSuffix: string,
): Promise<QueueInfo> => {
  const queue = getOrCreateQueue(name, connection, prefix)
  const [isPaused, counts] = await Promise.all([queue.isPaused(), queue.getJobCounts()])

  return {
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
  }
}

type StreamQueueInfo = QueueInfo & { id: string }

const DEBOUNCE_MS = 500

export class QueuesStream extends StreamAdapter<StreamQueueInfo> {
  private connection: Redis
  private prefix: string
  private dlqSuffix: string
  private knownQueues: Set<string> = new Set()
  private onQueueUpdate?: (queueInfo: StreamQueueInfo) => void
  private lastStatsCache: Map<string, string> = new Map()
  private debounceTimers: Map<string, NodeJS.Timeout> = new Map()

  constructor(connection: Redis, prefix: string, dlqSuffix: string) {
    super('__motia.bullmq-queues')
    this.connection = connection
    this.prefix = prefix
    this.dlqSuffix = dlqSuffix
  }

  setUpdateCallback(callback: (queueInfo: StreamQueueInfo) => void): void {
    this.onQueueUpdate = callback
  }

  async get(_groupId: string, id: string): Promise<StreamQueueInfo | null> {
    try {
      const info = await getQueueInfo(id, this.connection, this.prefix, this.dlqSuffix)
      return { ...info, id: info.name }
    } catch {
      return null
    }
  }

  async set(_groupId: string, _id: string, data: StreamQueueInfo): Promise<StreamQueueInfo> {
    return data
  }

  async delete(_groupId: string, id: string): Promise<StreamQueueInfo | null> {
    this.knownQueues.delete(id)
    this.lastStatsCache.delete(id)
    return { id } as StreamQueueInfo
  }

  async getGroup(_groupId: string): Promise<StreamQueueInfo[]> {
    const queueNames = await discoverQueueNames(this.connection, this.prefix)
    const queueInfos: StreamQueueInfo[] = []

    for (const name of queueNames) {
      this.knownQueues.add(name)
      const info = await getQueueInfo(name, this.connection, this.prefix, this.dlqSuffix)
      const streamInfo = { ...info, id: info.name }
      queueInfos.push(streamInfo)
      this.lastStatsCache.set(name, JSON.stringify({ stats: info.stats, isPaused: info.isPaused }))
    }

    return queueInfos
  }

  async refreshQueue(name: string): Promise<StreamQueueInfo> {
    const info = await getQueueInfo(name, this.connection, this.prefix, this.dlqSuffix)
    const streamInfo = { ...info, id: info.name }
    this.knownQueues.add(name)

    const newStatsKey = JSON.stringify({ stats: info.stats, isPaused: info.isPaused })
    const lastStatsKey = this.lastStatsCache.get(name)

    if (lastStatsKey !== newStatsKey) {
      this.lastStatsCache.set(name, newStatsKey)
      this.onQueueUpdate?.(streamInfo)
    }

    return streamInfo
  }

  private debouncedRefresh(queueName: string): void {
    const existing = this.debounceTimers.get(queueName)
    if (existing) {
      clearTimeout(existing)
    }

    const timer = setTimeout(() => {
      this.debounceTimers.delete(queueName)
      this.refreshQueue(queueName)
    }, DEBOUNCE_MS)

    this.debounceTimers.set(queueName, timer)
  }

  setupQueueEvents(queueName: string): void {
    if (queueEvents.has(queueName)) {
      return
    }

    const events = new QueueEvents(queueName, {
      connection: this.connection,
      prefix: this.prefix,
    })

    const refreshOnEvent = () => {
      this.debouncedRefresh(queueName)
    }

    events.on('waiting', refreshOnEvent)
    events.on('active', refreshOnEvent)
    events.on('completed', refreshOnEvent)
    events.on('failed', refreshOnEvent)
    events.on('delayed', refreshOnEvent)
    events.on('removed', refreshOnEvent)
  }

  async setupAllQueueEvents(): Promise<void> {
    const queueNames = await discoverQueueNames(this.connection, this.prefix)
    for (const name of queueNames) {
      this.setupQueueEvents(name)
    }
  }

  async closeAllQueueEvents(): Promise<void> {
    for (const timer of this.debounceTimers.values()) {
      clearTimeout(timer)
    }
    this.debounceTimers.clear()

    for (const [, events] of queueEvents) {
      await events.close()
    }
    queueEvents.clear()
  }

  getKnownQueues(): Set<string> {
    return this.knownQueues
  }
}
