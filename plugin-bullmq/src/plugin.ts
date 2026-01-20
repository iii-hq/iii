import type { MotiaPlugin, MotiaPluginContext } from '@motiadev/core'
import IORedis from 'ioredis'
import { api } from './api'
import { QueuesStream } from './streams/queues-stream'

const STREAM_NAME = '__motia.bullmq-queues'

const isBullMQAdapter = (adapter: unknown): adapter is { connection: IORedis; prefix: string; dlqSuffix: string } => {
  return (
    adapter !== null &&
    typeof adapter === 'object' &&
    'connection' in adapter &&
    'prefix' in adapter &&
    'dlqSuffix' in adapter
  )
}

export default function plugin(motia: MotiaPluginContext): MotiaPlugin {
  let connection: IORedis
  let prefix: string
  let dlqSuffix: string
  let ownsConnection = false

  if (isBullMQAdapter(motia.eventAdapter)) {
    connection = motia.eventAdapter.connection as IORedis
    prefix = motia.eventAdapter.prefix
    dlqSuffix = motia.eventAdapter.dlqSuffix
  } else {
    const host = process.env.BULLMQ_REDIS_HOST || process.env.REDIS_HOST || 'localhost'
    const port = parseInt(process.env.BULLMQ_REDIS_PORT || process.env.REDIS_PORT || '6379', 10)
    const password = process.env.BULLMQ_REDIS_PASSWORD || process.env.REDIS_PASSWORD || undefined
    prefix = process.env.BULLMQ_PREFIX || 'motia:events'
    dlqSuffix = process.env.BULLMQ_DLQ_SUFFIX || '.dlq'

    connection = new IORedis({ host, port, password, maxRetriesPerRequest: null })
    ownsConnection = true
  }

  const queuesStream = new QueuesStream(connection, prefix, dlqSuffix)

  const stream = motia.lockedData.createStream({
    filePath: `${STREAM_NAME}.ts`,
    hidden: true,
    config: {
      name: STREAM_NAME,
      baseConfig: { storageType: 'custom', factory: () => queuesStream },
      schema: null as never,
    },
  })()

  queuesStream.setUpdateCallback((queueInfo) => {
    stream.set('default', queueInfo.id, queueInfo)
  })

  queuesStream.setupAllQueueEvents().then(() => {
    queuesStream.getGroup('default').then((queues) => {
      for (const queue of queues) {
        stream.set('default', queue.id, queue)
      }
    })
  })

  api(motia, prefix, dlqSuffix, connection)

  return {
    workbench: [
      {
        packageName: '@motiadev/plugin-bullmq',
        cssImports: ['@motiadev/plugin-bullmq/dist/styles.css'],
        label: 'Queues',
        position: 'top',
        componentName: 'QueuesPage',
        labelIcon: 'layers',
      },
    ],
    onShutdown: async () => {
      await queuesStream.closeAllQueueEvents()
      if (ownsConnection) {
        await connection.quit()
      }
    },
  }
}
