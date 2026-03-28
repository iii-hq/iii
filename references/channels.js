/**
 * Pattern: Channels
 * Comparable to: Unix pipes, gRPC streaming, WebSocket data streams
 *
 * Demonstrates binary streaming between workers: creating channels,
 * passing refs across functions, writing/reading binary data, and
 * using text messages for signaling.
 *
 * How-to references:
 *   - Channels: https://iii.dev/docs/how-to/use-channels
 */

import { registerWorker, Logger } from 'iii-sdk'

const iii = registerWorker(process.env.III_ENGINE_URL || 'ws://localhost:49134', {
  workerName: 'channels-example',
})

// ---------------------------------------------------------------------------
// 1. Producer — creates a channel and streams data through it
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'pipeline::produce' }, async (data) => {
  const logger = new Logger()

  // Create a channel pair
  const channel = await iii.createChannel()

  // Pass the reader ref to the consumer via trigger
  iii.trigger({
    function_id: 'pipeline::consume',
    payload: {
      readerRef: channel.readerRef,
      recordCount: data.records.length,
    },
  })

  // Send metadata as a text message
  channel.writer.sendMessage(
    JSON.stringify({ type: 'metadata', format: 'ndjson', encoding: 'utf-8' }),
  )

  // Stream records as binary data (newline-delimited JSON)
  for (const record of data.records) {
    const line = JSON.stringify(record) + '\n'
    channel.writer.stream.write(Buffer.from(line))
  }

  // Signal end of stream
  channel.writer.close()
  logger.info('Producer finished streaming', { records: data.records.length })

  return { status: 'streaming', readerRef: channel.readerRef }
})

// ---------------------------------------------------------------------------
// 2. Consumer — receives a channel ref and reads the stream
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'pipeline::consume' }, async (data) => {
  const logger = new Logger()

  // Reconstruct reader from the ref passed in the payload
  const reader = data.readerRef

  // Listen for text messages (metadata, signaling)
  reader.onMessage((msg) => {
    const parsed = JSON.parse(msg)
    logger.info('Received metadata', parsed)
  })

  // Read entire binary stream
  const buffer = await reader.readAll()
  const text = buffer.toString('utf-8').trim()
  
  let records
  if (!text) {
    records = [{ processed: 0 }]
  } else {
    const lines = text.split('\n').filter((line) => line.trim() !== '')
    records = lines.map((line) => JSON.parse(line))
  }

  logger.info('Consumer processed records', { count: records.length })
  return { processed: records.length }
})

// ---------------------------------------------------------------------------
// 3. HTTP trigger to kick off the pipeline
// ---------------------------------------------------------------------------
iii.registerTrigger({
  type: 'http',
  function_id: 'pipeline::produce',
  config: { api_path: '/pipeline/start', http_method: 'POST' },
})
