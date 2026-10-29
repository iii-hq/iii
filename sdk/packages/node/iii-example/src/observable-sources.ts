/**
 * Examples of observable sources: functions whose handler returns the event
 * they are called with, observed with args, and the built-in sources.
 *
 * Shows three patterns:
 * 1. A source with args: `args_format` for the observers' args, hooks that set
 *    something up per observer, and keyed delivery
 * 2. Observing the built-in sources (cron, state, pubsub) with args or filters
 * 3. Listing the functions and how many observers each has
 */

import { keyed, ObserveRejection } from 'iii-sdk'
import { EngineFunctions } from 'iii-sdk/engine'
import { iii } from './iii'

/** Minimal row type for `engine::functions::list` responses. */
type FunctionRow = {
  function_id: string
  description?: string
  args_format?: Record<string, unknown>
  private?: boolean
  observer_count: number
}

// ── 1. Webhook source ────────────────────────────────────────────────────

type WebhookArgs = {
  /** URL path for incoming webhooks */
  url: string
  /** HMAC secret for signature verification */
  secret?: string
  /** HTTP methods to accept */
  methods?: string[]
}

type WebhookEvent = {
  /** The URL path the webhook arrived on */
  url: string
  /** HTTP method of the incoming webhook */
  method: string
  /** Request headers */
  headers: Record<string, string>
  /** Request body */
  body: unknown
  /** Whether the HMAC signature was verified */
  signature_verified: boolean
}

const listeners = new Map<string, number>() // url -> observers listening on it

const webhooks = iii
  .registerFunction('webhooks::on-received', (event: WebhookEvent) => event, {
    description: 'One call per incoming webhook. Observe it with args { url, secret?, methods? }.',
    args_format: {
      type: 'object',
      additionalProperties: false,
      required: ['url'],
      properties: {
        url: { type: 'string', pattern: '^/' },
        secret: { type: 'string' },
        methods: { type: 'array', items: { type: 'string' } },
      },
    },
    private: true,
  })
  .onObserverAdded(obs => {
    const { url } = obs.args as WebhookArgs
    if (url.startsWith('/internal/'))
      throw new ObserveRejection('URL_RESERVED', `${url} is reserved`)
    listeners.set(url, (listeners.get(url) ?? 0) + 1)
    console.log(`[webhooks] listening on ${url} for ${obs.target}`)
  })
  .onObserverRemoved(obs => {
    const { url } = obs.args as WebhookArgs
    const left = (listeners.get(url) ?? 1) - 1
    if (left === 0) listeners.delete(url)
    else listeners.set(url, left)
  })
  // each webhook reaches the observers of its url
  .onObserverDeliver(
    keyed<WebhookEvent, WebhookEvent>({
      observer: obs => (obs.args as WebhookArgs).url,
      call: call => call.input.url,
    }),
  )

/** What the webhook server calls for each request; nothing is sent when nobody listens. */
export async function onWebhook(event: WebhookEvent): Promise<void> {
  if (!webhooks.isObserved({ input: event })) return
  await iii.call({ function_id: 'webhooks::on-received', payload: event })
}

iii.registerFunction('example::webhook_handler', async (event: WebhookEvent) => ({
  processed: true,
  method: event.method,
}))

iii.observe(
  {
    function_id: 'webhooks::on-received',
    args: { url: '/hooks/my-service', secret: 'my-secret-key', methods: ['POST', 'PUT'] },
    deliver: 'input',
  },
  'example::webhook_handler',
)

// ── 2. Built-in sources ──────────────────────────────────────────────────

iii.registerFunction('example::scheduled_cleanup', async () => ({ cleaned: true }))
iii.observe(
  { function_id: 'cron::on-tick', args: { expression: '0 * * * * *' } },
  'example::scheduled_cleanup',
)

iii.registerFunction('example::on_user_updated', async (event: { event_type?: string }) => ({
  processed: true,
  event: event.event_type,
}))
iii.observe(
  {
    function_id: 'state::on-changed',
    filters: { 'input.scope': { $eq: 'users' } },
    deliver: 'input',
  },
  'example::on_user_updated',
)

iii.registerFunction(
  'example::on_order_created',
  async (message: { topic: string; data: unknown }) => ({
    processed: true,
    order: message.data,
  }),
)
iii.observe(
  {
    function_id: 'pubsub::publish',
    filters: { 'input.topic': { $eq: 'orders.created' } },
    deliver: 'input',
  },
  'example::on_order_created',
)

// ── 3. List functions and their observers ────────────────────────────────

export async function printObservableFunctions() {
  console.log('\n--- Functions and their observers ---')

  const { functions } = await iii.call<{ include_internal: boolean }, { functions: FunctionRow[] }>(
    {
      function_id: EngineFunctions.LIST_FUNCTIONS,
      payload: { include_internal: false },
    },
  )

  for (const fn of functions) {
    const args = fn.args_format ? ' (takes args)' : ''
    console.log(
      `  [${fn.function_id}] ${fn.observer_count} observer(s)${args} ${fn.description ?? ''}`,
    )
  }
}
