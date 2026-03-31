/**
 * Pattern: Low-Code/No-Code Workflow Builders
 * Comparable to: n8n, Zapier, LangFlow
 *
 * Demonstrates simple trigger → transform → action chains.
 * Each "node" in the automation is a small registered function.
 * Automations are chained via named queues, making it easy to
 * add/remove/reorder steps.
 *
 * How-to references:
 *   - Functions & Triggers: https://iii.dev/docs/how-to/use-functions-and-triggers
 *   - HTTP endpoints:       https://iii.dev/docs/how-to/expose-http-endpoint
 *   - Queues:               https://iii.dev/docs/how-to/use-queues
 *   - PubSub:               https://iii.dev/docs/how-to/use-functions-and-triggers
 *   - Cron:                 https://iii.dev/docs/how-to/schedule-cron-task
 */

import { registerWorker, Logger, TriggerAction } from 'iii-sdk'

const iii = registerWorker(process.env.III_ENGINE_URL || 'ws://localhost:49134', {
  workerName: 'low-code-automation',
})

// ===================================================================
// Automation 1: "When a form is submitted → enrich → store → notify"
// (Like a Zapier zap: Typeform → Clearbit → Google Sheets → Slack)
// ===================================================================

// Node: Webhook trigger (incoming form data)
iii.registerFunction({ id: 'auto::form-webhook' }, async (data) => {
  iii.trigger({
    function_id: 'auto::enrich-lead',
    payload: {
      submission_id: `sub-${Date.now()}`,
      email: data.email,
      company: data.company,
      message: data.message,
      received_at: new Date().toISOString(),
    },
    action: TriggerAction.Enqueue({ queue: 'automation' }),
  })
  return { status: 'accepted' }
})

iii.registerTrigger({
  type: 'http',
  function_id: 'auto::form-webhook',
  config: { api_path: '/webhooks/form', http_method: 'POST' },
})

// Node: Enrich the lead data
iii.registerFunction({ id: 'auto::enrich-lead' }, async (data) => {
  const logger = new Logger()
  logger.info('Enriching lead', { email: data.email })

  const enriched = {
    ...data,
    company_size: 'mid-market',
    industry: 'technology',
    enriched: true,
  }

  iii.trigger({
    function_id: 'auto::store-lead',
    payload: enriched,
    action: TriggerAction.Enqueue({ queue: 'automation' }),
  })

  return enriched
})

// Node: Store in "spreadsheet" (state)
iii.registerFunction({ id: 'auto::store-lead' }, async (data) => {
  await iii.trigger({ function_id: 'state::set', payload: {
    scope: 'leads',
    key: data.submission_id,
    value: { _key: data.submission_id, ...data },
  } })

  iii.trigger({
    function_id: 'auto::notify-team',
    payload: data,
    action: TriggerAction.Enqueue({ queue: 'automation' }),
  })
})

// Node: Send a Slack-like notification
iii.registerFunction({ id: 'auto::notify-team' }, async (data) => {
  const logger = new Logger()
  logger.info('Notifying team about new lead', {
    email: data.email,
    company: data.company,
  })

  // In production, this would call a Slack webhook or similar
  iii.trigger({ function_id: 'publish', payload: {
    topic: 'notifications.internal',
    data: {
      channel: '#leads',
      text: `New lead from ${data.email} at ${data.company} (${data.company_size})`,
    },
  }, action: TriggerAction.Void() })
})

// ===================================================================
// Automation 2: "Every morning → pull metrics → format → email digest"
// (Like a Zapier schedule: Schedule → HTTP → Formatter → Gmail)
// ===================================================================

iii.registerFunction({ id: 'auto::daily-digest' }, async () => {
  const logger = new Logger()

  // Pull all leads from the "spreadsheet"
  const leads = await iii.trigger({ function_id: 'state::list', payload: { scope: 'leads' } })

  const today = new Date().toISOString().split('T')[0]
  const todayLeads = leads.filter((l) =>
    l.received_at?.startsWith(today)
  )

  const digest = {
    date: today,
    total_leads: leads.length,
    new_today: todayLeads.length,
    top_companies: todayLeads.map((l) => l.company).filter(Boolean),
  }

  logger.info('Daily digest generated', digest)

  iii.trigger({ function_id: 'publish', payload: {
    topic: 'notifications.internal',
    data: {
      channel: '#daily-digest',
      text: `Daily Report (${today}): ${digest.new_today} new leads, ${digest.total_leads} total`,
    },
  }, action: TriggerAction.Void() })

  return digest
})

iii.registerTrigger({
  type: 'cron',
  function_id: 'auto::daily-digest',
  config: { expression: '0 0 8 * * * *' }, // 8 AM daily
})
