/**
 * Examples demonstrating trigger type configuration formats and listing.
 *
 * Shows three patterns:
 * 1. Custom trigger type with typed config — returns a TriggerTypeRef handle
 * 2. Built-in trigger types (cron, state, subscribe) with raw config
 * 3. Listing all available trigger types with their schemas
 */

import type { TriggerConfig, TriggerHandler, TriggerTypeRef } from 'iii-sdk'
import { iii } from './iii'

// ── Webhook trigger type ─────────────────────────────────────────────────

type WebhookTriggerConfig = {
  /** URL path for incoming webhooks */
  url: string
  /** HMAC secret for signature verification */
  secret?: string
  /** HTTP methods to accept */
  methods?: string[]
}

type WebhookCallRequest = {
  /** HTTP method of the incoming webhook */
  method: string
  /** Request headers */
  headers: Record<string, string>
  /** Request body */
  body: unknown
  /** Whether the HMAC signature was verified */
  signature_verified: boolean
}

const webhookHandler: TriggerHandler<WebhookTriggerConfig> = {
  async registerTrigger(config: TriggerConfig<WebhookTriggerConfig>) {
    console.log(`[webhook] Registered trigger ${config.id} -> ${config.function_id}`)
  },
  async unregisterTrigger(config: TriggerConfig<WebhookTriggerConfig>) {
    console.log(`[webhook] Unregistered trigger ${config.id}`)
  },
}

// ── Schedule trigger type ────────────────────────────────────────────────

type ScheduleTriggerConfig = {
  /** ISO 8601 datetime for when to fire */
  at: string
  /** Timezone (defaults to UTC) */
  timezone?: string
  /** Repeat at the same time daily */
  repeat_daily?: boolean
}

const scheduleHandler: TriggerHandler<ScheduleTriggerConfig> = {
  async registerTrigger(config: TriggerConfig<ScheduleTriggerConfig>) {
    console.log(`[schedule] Registered: ${JSON.stringify(config.config)}`)
  },
  async unregisterTrigger() {},
}

// ── Setup ────────────────────────────────────────────────────────────────

// 1. Webhook: typed handle with registerFunction + registerTrigger

const webhook: TriggerTypeRef<WebhookTriggerConfig> = iii.registerTriggerType(
  { id: 'webhook', description: 'Incoming webhook trigger' },
  webhookHandler,
)

// registerFunction on the handle: registers function + trigger in one call
webhook.registerFunction(
  { id: 'example::webhook_handler' },
  async (data: WebhookCallRequest) => ({
    processed: true,
    method: data.method,
  }),
  { url: '/hooks/my-service', secret: 'my-secret-key', methods: ['POST', 'PUT'] },
)

// registerTrigger on the handle: no need to pass `type`
webhook.registerTrigger('example::webhook_handler', {
  url: '/hooks/other-endpoint',
  methods: ['POST'],
})

// 2. Schedule: same pattern

const schedule: TriggerTypeRef<ScheduleTriggerConfig> = iii.registerTriggerType(
  { id: 'schedule', description: 'One-time or daily scheduled trigger' },
  scheduleHandler,
)

schedule.registerFunction(
  { id: 'example::send_report' },
  async () => ({ sent: true }),
  { at: '2026-03-25T09:00:00Z', timezone: 'America/Sao_Paulo', repeat_daily: true },
)

// 3. Built-in trigger types (cron, state, subscribe) — use iii directly

iii.registerFunction({ id: 'example::scheduled_cleanup' }, async (data: { job_id?: string }) => ({
  cleaned: true,
  job_id: data.job_id,
}))
iii.registerTrigger({
  type: 'cron',
  function_id: 'example::scheduled_cleanup',
  config: { expression: '0 * * * * *' },
})

iii.registerFunction({ id: 'example::on_user_updated' }, async (data: { event_type?: string }) => ({
  processed: true,
  event: data.event_type,
}))
iii.registerTrigger({
  type: 'state',
  function_id: 'example::on_user_updated',
  config: { scope: 'users' },
})

iii.registerFunction({ id: 'example::on_order_created' }, async (data: Record<string, unknown>) => ({
  processed: true,
  order: data,
}))
iii.registerTrigger({
  type: 'subscribe',
  function_id: 'example::on_order_created',
  config: { topic: 'orders.created' },
})

// ── List trigger types ───────────────────────────────────────────────────

export async function listTriggerTypesExample() {
  console.log('\n--- Listing all trigger types ---')

  const triggerTypes = await iii.listTriggerTypes()

  console.log(`Found ${triggerTypes.length} trigger types:\n`)
  for (const tt of triggerTypes) {
    console.log(`  [${tt.id}] ${tt.description}`)
    if (tt.trigger_request_format) {
      console.log(`    trigger_request_format: ${JSON.stringify(tt.trigger_request_format)}`)
    }
    if (tt.call_request_format) {
      console.log(`    call_request_format: ${JSON.stringify(tt.call_request_format)}`)
    }
    console.log()
  }
}
