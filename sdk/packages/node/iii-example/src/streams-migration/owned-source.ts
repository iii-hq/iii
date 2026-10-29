/**
 * Recipe 1 of 3: ephemeral change and lifecycle notifications.
 *
 * Replaces: calling the iii-stream `set` / `send` functions only so that someone hears
 * about a change, and observing iii-stream changes (or the dedicated Streams WebSocket)
 * to hear it.
 *
 * With: an observable source OWNED by the domain worker (`orders::on-changed`) plus a
 * consumer function observing it. The source is a private function whose handler returns
 * the event it is called with; the engine validates each observer's args against its
 * `args_format`, and matching by keys hands each event to the observers of its tenant, order
 * and event. Payloads are small (ids only); a consumer that needs the record asks the
 * owner (see stored-records.ts).
 *
 * A notification is not a database or an event log: nothing here is stored or replayed.
 * A consumer that is not observing when an event happens never sees it.
 *
 * Run against an engine (iii-stream NOT required):
 *   III_URL=ws://127.0.0.1:49134 node src/streams-migration/owned-source.ts
 *
 * Migration guide: https://iii.dev/docs/upgrading/migrate-from-streams
 */
import { type IIIClient, ObserveRejection, registerWorker } from 'iii-sdk'

const ENGINE_URL = process.env.III_URL ?? 'ws://127.0.0.1:49134'
// Distinct worker names per run: the engine keeps one live worker per (namespace, name).
const RUN_ID = Math.random().toString(36).slice(2, 8)

// ── Contract of the source ──────────────────────────────────────────────────

export const ORDER_CHANGED = 'orders::on-changed'
const ORDER_EVENTS = ['created', 'paid', 'cancelled'] as const
export type OrderEvent = (typeof ORDER_EVENTS)[number]

/**
 * An observer's args. `tenant_id` is REQUIRED: the provider only ever delivers a tenant's
 * events to observers of that tenant. This replaces the isolation that stream groups and
 * `stream:join` / `stream:leave` / the Streams `auth_function` used to provide. WHO may
 * observe a tenant is a deployment policy: enforce it with worker-manager RBAC, not with
 * a transport.
 */
export type OrderChangedArgs = {
  tenant_id: string
  order_id?: string
  events?: OrderEvent[]
}

/** The event: small and id-only. Never put whole datasets in an event. */
export type OrderChangedEvent = {
  event: OrderEvent
  tenant_id: string
  order_id: string
  occurred_at: number
}

/** Upper bound on observers; more are rejected, not queued. */
const MAX_OBSERVERS = 256

/** Validated by the engine: an observer with other args is refused with INVALID_ARGS. */
const ARGS_FORMAT = {
  type: 'object' as const,
  additionalProperties: false,
  required: ['tenant_id'],
  properties: {
    tenant_id: { type: 'string', minLength: 1 },
    order_id: { type: 'string' },
    events: { type: 'array', items: { type: 'string', enum: [...ORDER_EVENTS] } },
  },
}

/** Match keys: an observer is filed under its tenant, order (or `*`) and events (or `*`). */
const key = (tenant: string, order: string, event: string) => `${tenant}|${order}|${event}`

// ── Provider: the orders domain worker ──────────────────────────────────────

export function startOrdersWorker(engineUrl = ENGINE_URL) {
  const iii = registerWorker(engineUrl, {
    workerName: `orders-${RUN_ID}`,
    otel: { enabled: false },
  })

  const changed = iii
    .registerFunction(ORDER_CHANGED, (event: OrderChangedEvent) => event, {
      description:
        'Called when an order changes state. Observe it with args { tenant_id (required), order_id?, events? }. ' +
        'Event: { event, tenant_id, order_id, occurred_at }. Ephemeral: not stored, not replayed.',
      request_format: {
        type: 'object',
        required: ['event', 'tenant_id', 'order_id', 'occurred_at'],
        properties: {
          event: { type: 'string', enum: [...ORDER_EVENTS] },
          tenant_id: { type: 'string' },
          order_id: { type: 'string' },
          occurred_at: { type: 'integer' },
        },
      },
      args_format: ARGS_FORMAT,
      private: true,
    })
    .onObserverAdded(obs => {
      if (changed.observers().length >= MAX_OBSERVERS) {
        throw new ObserveRejection(
          'OBSERVER_LIMIT',
          `${ORDER_CHANGED}: observer limit (${MAX_OBSERVERS}) reached`,
        )
      }
      const args = obs.args as OrderChangedArgs
      console.log(`[orders] observed by ${obs.id} -> ${obs.target} (tenant=${args.tenant_id})`)
    })
    .onObserverRemoved(obs => {
      // Runs on dispose, and when the observing worker's observers are removed.
      console.log(`[orders] observer ${obs.id} removed`)
    })
    .onObserverMatch({
      observer: ({ args }) => {
        const { tenant_id, order_id, events } = args as OrderChangedArgs
        return (events ?? ['*']).map(event => key(tenant_id, order_id ?? '*', event))
      },
      call: ({ input }) =>
        [input.order_id, '*'].flatMap(order =>
          [input.event, '*'].map(event => key(input.tenant_id, order, event)),
        ),
    })

  // A domain operation. The event describes something that already happened.
  iii.registerFunction(
    'orders::transition',
    async (input: { tenant_id: string; order_id: string; event: OrderEvent }) => {
      if (!(ORDER_EVENTS as readonly string[]).includes(input.event))
        throw new Error('invalid event')
      // ... perform the transition (charge card, cancel shipment, ...) ...
      const event: OrderChangedEvent = { ...input, occurred_at: Date.now() }
      // Skip building and sending an event nobody observes.
      const observed = changed.isObserved({ input: event })
      if (observed) await iii.call({ function_id: ORDER_CHANGED, payload: event })
      return { observed }
    },
    {
      description: 'Apply an order transition and notify the observers of orders::on-changed',
      request_format: {
        type: 'object',
        required: ['tenant_id', 'order_id', 'event'],
        properties: {
          tenant_id: { type: 'string' },
          order_id: { type: 'string' },
          event: { type: 'string', enum: [...ORDER_EVENTS] },
        },
      },
      response_format: { type: 'object', properties: { observed: { type: 'boolean' } } },
    },
  )

  return { iii, observerCount: () => changed.observers().length }
}

// ── Consumer: a billing worker observing the provider's source ──────────────

export function startBillingWorker(tenantId: string, engineUrl = ENGINE_URL) {
  const iii = registerWorker(engineUrl, {
    workerName: `billing-${RUN_ID}`,
    otel: { enabled: false },
  })
  const received: Array<{ event: OrderChangedEvent; metadata: unknown }> = []

  iii.registerFunction('billing::on-order-changed', async (event: OrderChangedEvent, metadata) => {
    received.push({ event, metadata })
    console.log(
      `[billing] ${event.event} ${event.order_id} (tenant ${event.tenant_id}) metadata=${JSON.stringify(metadata)}`,
    )
    return null
  })

  const observer = iii.observe(
    {
      function_id: ORDER_CHANGED,
      args: { tenant_id: tenantId, events: ['created', 'cancelled'] } satisfies OrderChangedArgs,
      deliver: 'input', // the target receives the event itself
    },
    'billing::on-order-changed',
    { metadata: { subscriber: 'billing' } }, // the metadata sidecar of every activation
  )

  return { iii, received, observer }
}

// ── Demo: run both against one engine and check the observable behaviour ────

const sleep = (ms: number) => new Promise(resolve => setTimeout(resolve, ms))

async function waitFor(label: string, condition: () => boolean, timeoutMs = 5000): Promise<void> {
  const start = Date.now()
  while (!condition()) {
    if (Date.now() - start > timeoutMs) throw new Error(`timed out waiting for: ${label}`)
    await sleep(25)
  }
}

function check(condition: boolean, message: string): void {
  if (!condition) throw new Error(`check failed: ${message}`)
  console.log(`[demo] ok: ${message}`)
}

async function transition(
  caller: IIIClient,
  tenant_id: string,
  order_id: string,
  event: OrderEvent,
) {
  const { observed } = await caller.call<unknown, { observed: boolean }>({
    function_id: 'orders::transition',
    payload: { tenant_id, order_id, event },
  })
  console.log(
    `[demo] orders::transition ${tenant_id}/${order_id} ${event} -> observed: ${observed}`,
  )
  return observed
}

async function main(): Promise<void> {
  console.log(`[demo] engine ${ENGINE_URL}`)
  const orders = startOrdersWorker()
  const billing = startBillingWorker('tenant-1')

  try {
    await billing.observer.ready()
    check(orders.observerCount() === 1, 'the provider accepted the billing observer')

    // Invalid args: the engine refuses the observer against args_format, before any hook.
    const invalid = billing.iii.observe(
      { function_id: ORDER_CHANGED, args: { tenant_id: '', events: ['shipped'] } },
      'billing::on-order-changed',
    )
    const refusal = await invalid.ready().then(
      () => undefined,
      (error: { code?: string }) => error.code,
    )
    check(refusal === 'INVALID_ARGS', 'invalid args were refused (INVALID_ARGS)')
    check(orders.observerCount() === 1, 'still 1 observer')

    await transition(billing.iii, 'tenant-1', 'order-1', 'created') // delivered
    await transition(billing.iii, 'tenant-1', 'order-1', 'paid') // not one of the observer's events
    await transition(billing.iii, 'tenant-2', 'order-9', 'created') // other tenant: never delivered
    await transition(billing.iii, 'tenant-1', 'order-1', 'cancelled') // delivered

    await waitFor('two notifications', () => billing.received.length >= 2)
    await sleep(300) // make sure nothing else trickles in
    check(billing.received.length === 2, 'exactly the 2 matching notifications were received')
    check(
      billing.received
        .map(r => r.event.event)
        .sort()
        .join(',') === 'cancelled,created',
      'events args applied (created + cancelled, not paid)',
    )
    check(
      billing.received.every(r => r.event.tenant_id === 'tenant-1'),
      'tenant isolation (no tenant-2 event)',
    )
    check(
      billing.received.every(
        r => JSON.stringify(r.metadata) === JSON.stringify({ subscriber: 'billing' }),
      ),
      'observer metadata reaches the handler',
    )

    // Cleanup: disposing reaches the provider, which no longer delivers.
    await billing.observer.dispose()
    await waitFor('provider saw the dispose', () => orders.observerCount() === 0)
    check(
      !(await transition(billing.iii, 'tenant-1', 'order-2', 'created')),
      'nobody observes after dispose',
    )

    console.log('[demo] owned-source: PASS')
  } finally {
    await billing.iii.shutdown()
    await orders.iii.shutdown()
  }
}

main().then(
  () => process.exit(0),
  error => {
    console.error('[demo] owned-source: FAIL', error)
    process.exit(1)
  },
)
