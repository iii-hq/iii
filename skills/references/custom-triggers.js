/**
 * Pattern: Custom Triggers
 * Comparable to: Custom event adapters, webhook connectors, polling integrators
 *
 * Demonstrates how to define entirely new trigger types beyond the built-in
 * http, durable:subscriber, cron, state, and subscribe triggers. A custom trigger type
 * registers handler callbacks that the engine invokes when triggers of that
 * type are created or removed, letting you bridge any external event source
 * (webhooks, file-system watchers, pollers) into the iii function graph.
 *
 * How-to references:
 *   - Custom trigger types: https://iii.dev/docs/how-to/create-custom-trigger-type
 */

import { registerWorker, Logger, TriggerAction } from 'iii-sdk'

const iii = registerWorker(process.env.III_ENGINE_URL || 'ws://localhost:49134', {
  workerName: 'custom-triggers',
})

// ---------------------------------------------------------------------------
// Custom trigger type — Webhook receiver
// Registers an HTTP endpoint per trigger and fires the bound function when
// an external service POSTs to it.
// ---------------------------------------------------------------------------
const webhookEndpoints = new Map()

iii.registerTriggerType({
  id: 'webhook',
  description: 'Fires when an external service sends an HTTP POST to the registered endpoint',
  handler: {
    // Called when a trigger of this type is created via registerTrigger
    // TriggerConfig shape: { id, function_id, config }
    registerTrigger: async (triggerConfig) => {
      const logger = new Logger()
      const { id, function_id, config } = triggerConfig
      const path = config.path || `/webhooks/${id}`

      logger.info('Registering webhook endpoint', { id, path })

      // In a real implementation you would bind an HTTP route here.
      // When the route receives a POST the callback fires the bound function.
      const endpoint = {
        path,
        callback: async (requestBody) => {
          await iii.trigger({
            function_id,
            payload: { source: 'webhook', trigger_id: id, data: requestBody },
          })
        },
      }

      webhookEndpoints.set(id, endpoint)
    },

    // Called when the trigger is removed — clean up the endpoint
    unregisterTrigger: async (triggerConfig) => {
      const logger = new Logger()
      logger.info('Removing webhook endpoint', { id: triggerConfig.id })
      webhookEndpoints.delete(triggerConfig.id)
    },
  },
})

// ---------------------------------------------------------------------------
// Custom trigger type — File watcher
// Uses fs.watch to fire the bound function whenever a file changes.
// ---------------------------------------------------------------------------
import fs from 'fs'

const fileWatchers = new Map()

iii.registerTriggerType({
  id: 'file-watch',
  description: 'Fires when a file on the local filesystem changes',
  handler: {
    registerTrigger: async (triggerConfig) => {
      const { id, function_id, config } = triggerConfig
      const filePath = config.file_path

      const watcher = fs.watch(filePath, (eventType, filename) => {
        iii.trigger({
          function_id,
          payload: { source: 'file-watch', trigger_id: id, eventType, filename },
          action: TriggerAction.Void(), // fire-and-forget, don't block the watcher
        })
      })

      fileWatchers.set(id, watcher)
    },

    unregisterTrigger: async (triggerConfig) => {
      const watcher = fileWatchers.get(triggerConfig.id)
      if (watcher) {
        watcher.close()
        fileWatchers.delete(triggerConfig.id)
      }
    },
  },
})

// ---------------------------------------------------------------------------
// Custom trigger type — Polling with ETag
// Periodically fetches a URL and fires only when the content changes.
// ---------------------------------------------------------------------------
const pollers = new Map()

iii.registerTriggerType({
  id: 'polling',
  description: 'Polls a URL at a fixed interval and fires when the ETag changes',
  handler: {
    registerTrigger: async (triggerConfig) => {
      const { id, function_id, config } = triggerConfig
      const { url, interval_ms = 30000 } = config
      let lastETag = null

      const timer = setInterval(async () => {
        try {
          const res = await fetch(url, {
            method: 'GET',
            headers: lastETag ? { 'If-None-Match': lastETag } : {},
          })

          if (res.status === 304) return // no change

          const etag = res.headers.get('etag')
          if (etag && etag !== lastETag) {
            lastETag = etag
            const body = await res.json()

            await iii.trigger({
              function_id,
              payload: { source: 'polling', trigger_id: id, etag, data: body },
            })
          }
        } catch (err) {
          const logger = new Logger()
          logger.error('Polling failed', { id, url, error: err.message })
        }
      }, interval_ms)

      pollers.set(id, timer)
    },

    unregisterTrigger: async (triggerConfig) => {
      const timer = pollers.get(triggerConfig.id)
      if (timer) {
        clearInterval(timer)
        pollers.delete(triggerConfig.id)
      }
    },
  },
})

// ---------------------------------------------------------------------------
// Handler function — processes events from any custom trigger above
// ---------------------------------------------------------------------------
iii.registerFunction('custom-triggers::on-event', async (data) => {
  const logger = new Logger()
  logger.info('Custom trigger fired', { source: data.source, trigger_id: data.trigger_id })
  return { received: true, source: data.source }
})

// ---------------------------------------------------------------------------
// Bind triggers using the custom types defined above
// ---------------------------------------------------------------------------
iii.registerTrigger({
  type: 'webhook',
  function_id: 'custom-triggers::on-event',
  config: { path: '/hooks/github' },
})

iii.registerTrigger({
  type: 'file-watch',
  function_id: 'custom-triggers::on-event',
  config: { file_path: '/var/data/config.json' },
})

iii.registerTrigger({
  type: 'polling',
  function_id: 'custom-triggers::on-event',
  config: { url: 'https://api.example.com/status', interval_ms: 60000 },
})

// ---------------------------------------------------------------------------
// Cleanup — unregister a trigger type when it is no longer needed
// ---------------------------------------------------------------------------
// iii.unregisterTriggerType('polling')
