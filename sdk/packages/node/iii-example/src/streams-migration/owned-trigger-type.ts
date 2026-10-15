/**
 * Recipe 1 of 3: ephemeral change and lifecycle notifications.
 *
 * Replaces: calling the iii-stream `set` / `send` functions only so that someone hears
 * about a change, and binding the deprecated `stream` trigger type (or the dedicated Streams
 * WebSocket) to hear it.
 *
 * With: a trigger type OWNED by the domain worker (`orders::changed`) plus a consumer
 * function bound to it. The provider owns register/unregister, validates the filter,
 * forwards the binding's namespace and metadata, and bounds fan-out. Payloads are small
 * (ids only); a consumer that needs the record asks the owner (see stored-records.ts).
 *
 * A trigger is a notification mechanism, not a database or an event log: nothing here is
 * stored or replayed. A consumer that is not bound when an event fires never sees it.
 *
 * Run against an engine (iii-stream NOT required):
 *   III_URL=ws://127.0.0.1:49134 node src/streams-migration/owned-trigger-type.ts
 *
 * Migration guide: https://iii.dev/docs/upgrading/migrate-from-streams
 */
import { type IIIClient, registerWorker, TriggerAction } from 'iii-sdk'
import type { TriggerConfig } from 'iii-sdk/trigger'

const ENGINE_URL = process.env.III_URL ?? 'ws://127.0.0.1:49134'
// Distinct worker names per run: the engine keeps one live worker per (namespace, name).
const RUN_ID = Math.random().toString(36).slice(2, 8)

// ── Contract of the trigger type ────────────────────────────────────────────

export const ORDER_CHANGED = 'orders::changed'
const ORDER_EVENTS = ['created', 'paid', 'cancelled'] as const
export type OrderEvent = (typeof ORDER_EVENTS)[number]

/**
 * Binding filter. `tenant_id` is REQUIRED: the provider only ever delivers a tenant's
 * events to bindings for that tenant. This replaces the isolation that stream groups and
 * `stream:join` / `stream:leave` / the Streams `auth_function` used to provide. WHO may
 * bind for a tenant is a deployment policy: enforce it with worker-manager RBAC
 * (`on_trigger_registration_function_id`), not with a transport.
 */
export type OrderChangedConfig = {
  tenant_id: string
  order_id?: string
  events?: OrderEvent[]
  /** Optional metadata echoed to the handler (second argument). Wins over binding metadata. */
  metadata?: Record<string, unknown>
}

/** The notification: small and id-only. Never put whole datasets in a trigger payload. */
export type OrderChangedEvent = {
  event: OrderEvent
  tenant_id: string
  order_id: string
  occurred_at: number
}

/** Upper bound on live bindings; registrations beyond it are rejected, not queued. */
const MAX_BINDINGS = 256
const CONFIG_KEYS = new Set(['tenant_id', 'order_id', 'events', 'metadata'])

/** Validate a binding's filter. Throwing rejects the registration; the binder sees the error. */
function validateConfig(raw: unknown): OrderChangedConfig {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) {
    throw new Error(`${ORDER_CHANGED}: config must be an object`)
  }
  const config = raw as Record<string, unknown>
  for (const key of Object.keys(config)) {
    if (!CONFIG_KEYS.has(key)) throw new Error(`${ORDER_CHANGED}: unknown config key '${key}'`)
  }
  if (typeof config.tenant_id !== 'string' || config.tenant_id.length === 0) {
    throw new Error(`${ORDER_CHANGED}: tenant_id is required`)
  }
  if (config.order_id !== undefined && typeof config.order_id !== 'string') {
    throw new Error(`${ORDER_CHANGED}: order_id must be a string`)
  }
  if (config.events !== undefined) {
    const events = config.events
    if (
      !Array.isArray(events) ||
      events.some(e => !(ORDER_EVENTS as readonly unknown[]).includes(e))
    ) {
      throw new Error(`${ORDER_CHANGED}: events must be a subset of ${ORDER_EVENTS.join(', ')}`)
    }
  }
  if (
    config.metadata !== undefined &&
    (typeof config.metadata !== 'object' || config.metadata === null)
  ) {
    throw new Error(`${ORDER_CHANGED}: metadata must be an object`)
  }
  return config as OrderChangedConfig
}

function matches(config: OrderChangedConfig, event: OrderChangedEvent): boolean {
  if (config.tenant_id !== event.tenant_id) return false
  if (config.order_id !== undefined && config.order_id !== event.order_id) return false
  if (config.events !== undefined && !config.events.includes(event.event)) return false
  return true
}

// ── Provider: the orders domain worker ──────────────────────────────────────

export function startOrdersWorker(engineUrl = ENGINE_URL) {
  const iii = registerWorker(engineUrl, {
    workerName: `orders-${RUN_ID}`,
    otel: { enabled: false },
  })
  /** The provider's own registry of bindings, keyed by trigger id. */
  const bindings = new Map<string, TriggerConfig<OrderChangedConfig>>()

  iii.registerTriggerType<OrderChangedConfig>(
    {
      id: ORDER_CHANGED,
      description:
        'Fires when an order changes state. Config: { tenant_id (required), order_id?, events?, metadata? }. ' +
        'Payload: { event, tenant_id, order_id, occurred_at }. Ephemeral: not stored, not replayed.',
    },
    {
      async registerTrigger(binding) {
        const config = validateConfig(binding.config)
        if (!bindings.has(binding.id) && bindings.size >= MAX_BINDINGS) {
          throw new Error(`${ORDER_CHANGED}: binding limit (${MAX_BINDINGS}) reached`)
        }
        // Keep the whole binding: function_id, namespace and metadata are needed to deliver.
        bindings.set(binding.id, { ...binding, config })
        console.log(
          `[orders] bound ${binding.id} -> ${binding.function_id} (namespace=${binding.namespace ?? '(worker default)'}, tenant=${config.tenant_id})`,
        )
      },
      async unregisterTrigger(binding) {
        // Called on explicit unregister AND when the consumer disconnects.
        bindings.delete(binding.id)
        console.log(`[orders] unbound ${binding.id}`)
      },
    },
  )

  /**
   * Bounded fan-out: one fire-and-forget invocation per MATCHING binding (at most
   * MAX_BINDINGS), sent sequentially. No per-event task outlives this loop and nothing is
   * queued for slow consumers: the engine delivers, a slow handler cannot block the provider.
   */
  async function emit(event: OrderChangedEvent): Promise<number> {
    let delivered = 0
    for (const binding of Array.from(bindings.values())) {
      if (!matches(binding.config, event)) continue
      const metadata = binding.config.metadata ?? binding.metadata
      try {
        await iii.trigger({
          function_id: binding.function_id,
          namespace: binding.namespace, // preserve the binder's namespace
          payload: event,
          ...(metadata === undefined ? {} : { metadata }), // never drop the binder's metadata
          action: TriggerAction.Void(),
        })
        delivered++
      } catch (error) {
        console.warn(
          `[orders] delivery to ${binding.function_id} failed: ${(error as Error).message}`,
        )
      }
    }
    return delivered
  }

  // A domain operation. The notification describes something that already happened.
  iii.registerFunction(
    'orders::transition',
    async (input: { tenant_id: string; order_id: string; event: OrderEvent }) => {
      if (!(ORDER_EVENTS as readonly string[]).includes(input.event))
        throw new Error('invalid event')
      // ... perform the transition (charge card, cancel shipment, ...) ...
      const delivered = await emit({ ...input, occurred_at: Date.now() })
      return { delivered }
    },
    {
      description: 'Apply an order transition and notify orders::changed bindings',
      request_format: {
        type: 'object',
        required: ['tenant_id', 'order_id', 'event'],
        properties: {
          tenant_id: { type: 'string' },
          order_id: { type: 'string' },
          event: { type: 'string', enum: [...ORDER_EVENTS] },
        },
      },
      response_format: { type: 'object', properties: { delivered: { type: 'integer' } } },
    },
  )

  return { iii, bindingCount: () => bindings.size }
}

// ── Consumer: a billing worker bound to the provider's trigger type ─────────

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
    return null // Void delivery: the result is discarded
  })

  const binding = iii.registerTrigger({
    type: ORDER_CHANGED,
    function_id: 'billing::on-order-changed',
    config: { tenant_id: tenantId, events: ['created', 'cancelled'] } satisfies OrderChangedConfig,
    metadata: { subscriber: 'billing' },
  })

  return { iii, received, binding }
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
  const { delivered } = await caller.trigger<unknown, { delivered: number }>({
    function_id: 'orders::transition',
    payload: { tenant_id, order_id, event },
  })
  console.log(
    `[demo] orders::transition ${tenant_id}/${order_id} ${event} -> delivered to ${delivered} binding(s)`,
  )
  return delivered
}

async function main(): Promise<void> {
  console.log(`[demo] engine ${ENGINE_URL}`)
  const orders = startOrdersWorker()
  const billing = startBillingWorker('tenant-1')

  try {
    await waitFor('billing binding registered with the provider', () => orders.bindingCount() === 1)

    // Invalid filter: the provider throws in registerTrigger, the engine rejects the binding
    // and the binder's SDK logs "[iii] Trigger registration failed ...".
    billing.iii.registerTrigger({
      type: ORDER_CHANGED,
      function_id: 'billing::on-order-changed',
      config: { tenant_id: '', events: ['shipped'] },
    })
    await sleep(500)
    check(orders.bindingCount() === 1, 'invalid filter was rejected (still 1 binding)')

    await transition(billing.iii, 'tenant-1', 'order-1', 'created') // delivered
    await transition(billing.iii, 'tenant-1', 'order-1', 'paid') // filtered out by events
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
      'events filter applied (created + cancelled, not paid)',
    )
    check(
      billing.received.every(r => r.event.tenant_id === 'tenant-1'),
      'tenant isolation (no tenant-2 event)',
    )
    check(
      billing.received.every(
        r => JSON.stringify(r.metadata) === JSON.stringify({ subscriber: 'billing' }),
      ),
      'binding metadata forwarded to the handler',
    )

    // Cleanup: unregistering reaches the provider, which stops delivering.
    billing.binding.unregister()
    await waitFor('provider saw unregister', () => orders.bindingCount() === 0)
    check(
      (await transition(billing.iii, 'tenant-1', 'order-2', 'created')) === 0,
      'no delivery after unregister',
    )

    console.log('[demo] owned-trigger-type: PASS')
  } finally {
    await billing.iii.shutdown()
    await orders.iii.shutdown()
  }
}

main().then(
  () => process.exit(0),
  error => {
    console.error('[demo] owned-trigger-type: FAIL', error)
    process.exit(1)
  },
)
