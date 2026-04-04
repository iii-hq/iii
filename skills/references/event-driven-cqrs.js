/**
 * Pattern: Event-Driven / Message Systems (CQRS)
 * Comparable to: Kafka, RabbitMQ, CQRS/Event Sourcing systems
 *
 * Demonstrates CQRS (Command Query Responsibility Segregation) with
 * event sourcing. Commands publish domain events via pubsub. Multiple
 * read model projections subscribe independently. PubSub handles all
 * fan-out — both to projections and downstream notification consumers.
 *
 * How-to references:
 *   - Queues:           https://iii.dev/docs/how-to/use-queues
 *   - State management: https://iii.dev/docs/how-to/manage-state
 *   - State reactions:  https://iii.dev/docs/how-to/react-to-state-changes
 *   - HTTP endpoints:   https://iii.dev/docs/how-to/expose-http-endpoint
 *   - PubSub:           https://iii.dev/docs/how-to/use-functions-and-triggers
 */

import { registerWorker, Logger, TriggerAction } from 'iii-sdk'

const iii = registerWorker(process.env.III_ENGINE_URL || 'ws://localhost:49134', {
  workerName: 'event-driven-cqrs',
})

// ===================================================================
// WRITE SIDE — Commands that validate + emit domain events
// ===================================================================

// Command: Add item to inventory
iii.registerFunction('cmd::add-inventory-item', async (data) => {
  const logger = new Logger()
  const { sku, name, quantity, price } = data

  if (!sku || !name || quantity <= 0) {
    throw new Error('Invalid inventory item')
  }

  logger.info('Command: add inventory item', { sku })

  // Append to the event log (event sourcing)
  const event = {
    type: 'ItemAdded',
    sku,
    name,
    quantity,
    price,
    timestamp: new Date().toISOString(),
  }

  await appendEvent('inventory', sku, event)

  // Publish domain event for all projections to consume
  iii.trigger({ function_id: 'publish', payload: { topic: 'inventory.item-added', data: event }, action: TriggerAction.Void() })

  return { event: 'ItemAdded', sku }
})

// Command: Sell items (reduce stock)
iii.registerFunction('cmd::sell-item', async (data) => {
  const logger = new Logger()
  const { sku, quantity } = data

  // Read current state to validate
  const item = await iii.trigger({ function_id: 'state::get', payload: { scope: 'inventory-read', key: sku } })
  if (!item) throw new Error(`Item ${sku} not found`)
  if (item.stock < quantity) throw new Error(`Insufficient stock: ${item.stock} < ${quantity}`)

  logger.info('Command: sell item', { sku, quantity })

  const event = {
    type: 'ItemSold',
    sku,
    quantity,
    revenue: quantity * item.price,
    timestamp: new Date().toISOString(),
  }

  await appendEvent('inventory', sku, event)

  iii.trigger({ function_id: 'publish', payload: { topic: 'inventory.item-sold', data: event }, action: TriggerAction.Void() })

  return { event: 'ItemSold', sku, remaining: item.stock - quantity }
})

// HTTP command endpoints
iii.registerTrigger({ type: 'http', function_id: 'cmd::add-inventory-item', config: { api_path: '/inventory/add', http_method: 'POST' } })
iii.registerTrigger({ type: 'http', function_id: 'cmd::sell-item', config: { api_path: '/inventory/sell', http_method: 'POST' } })

// ===================================================================
// EVENT LOG — append-only event store (event sourcing)
// ===================================================================
async function appendEvent(aggregate, key, event) {
  const log = await iii.trigger({ function_id: 'state::get', payload: { scope: 'event-log', key: `${aggregate}:${key}` } })
  const events = log?.events || []
  events.push(event)

  await iii.trigger({ function_id: 'state::set', payload: {
    scope: 'event-log',
    key: `${aggregate}:${key}`,
    value: { _key: `${aggregate}:${key}`, events },
  } })
}

// ===================================================================
// READ SIDE — Projections that build query-optimized views from events
// ===================================================================

// Projection 1: Inventory catalog (current stock levels)
iii.registerFunction('proj::catalog-on-add', async (event) => {
  await iii.trigger({ function_id: 'state::set', payload: {
    scope: 'inventory-read',
    key: event.sku,
    value: {
      _key: event.sku,
      sku: event.sku,
      name: event.name,
      price: event.price,
      stock: event.quantity,
      last_updated: event.timestamp,
    },
  } })
})

iii.registerFunction('proj::catalog-on-sell', async (event) => {
  await iii.trigger({ function_id: 'state::update', payload: {
    scope: 'inventory-read',
    key: event.sku,
    ops: [
      { type: 'increment', path: 'stock', by: -event.quantity },
      { type: 'set', path: 'last_updated', value: event.timestamp },
    ],
  } })
})

// Projection 2: Sales analytics (aggregated metrics)
iii.registerFunction('proj::sales-analytics', async (event) => {
  await iii.trigger({ function_id: 'state::update', payload: {
    scope: 'sales-analytics',
    key: 'global',
    ops: [
      { type: 'increment', path: 'total_sales', by: 1 },
      { type: 'increment', path: 'total_revenue', by: event.revenue },
      { type: 'increment', path: `by_sku.${event.sku}`, by: event.quantity },
      { type: 'set', path: 'last_sale_at', value: event.timestamp },
    ],
  } })
})

// Projections subscribe to domain events independently via pubsub
iii.registerTrigger({ type: 'subscribe', function_id: 'proj::catalog-on-add', config: { topic: 'inventory.item-added' } })
iii.registerTrigger({ type: 'subscribe', function_id: 'proj::catalog-on-sell', config: { topic: 'inventory.item-sold' } })
iii.registerTrigger({ type: 'subscribe', function_id: 'proj::sales-analytics', config: { topic: 'inventory.item-sold' } })

// ===================================================================
// FAN-OUT — PubSub notifications to downstream systems
// ===================================================================
iii.registerFunction('notify::low-stock-alert', async (event) => {
  const item = await iii.trigger({ function_id: 'state::get', payload: { scope: 'inventory-read', key: event.sku } })
  if (item && item.stock <= 5) {
    iii.trigger({ function_id: 'publish', payload: {
      topic: 'alerts.low-stock',
      data: { sku: event.sku, name: item.name, remaining: item.stock },
    }, action: TriggerAction.Void() })
  }
})

iii.registerTrigger({
  type: 'subscribe',
  function_id: 'notify::low-stock-alert',
  config: { topic: 'inventory.item-sold' },
})

// Fan-out subscriber: could be a separate service listening for alerts
iii.registerFunction('notify::slack-low-stock', async (data) => {
  const logger = new Logger()
  logger.warn('LOW STOCK ALERT', { sku: data.sku, remaining: data.remaining })
  // In production: POST to Slack webhook, send email, page oncall, etc.
})

iii.registerTrigger({
  type: 'subscribe',
  function_id: 'notify::slack-low-stock',
  config: { topic: 'alerts.low-stock' },
})

// ===================================================================
// QUERY ENDPOINTS — read from projections (not the event log)
// ===================================================================
iii.registerFunction('query::catalog', async () => {
  return await iii.trigger({ function_id: 'state::list', payload: { scope: 'inventory-read' } })
})

iii.registerFunction('query::sales-analytics', async () => {
  return await iii.trigger({ function_id: 'state::get', payload: { scope: 'sales-analytics', key: 'global' } })
})

iii.registerFunction('query::event-history', async (data) => {
  const log = await iii.trigger({ function_id: 'state::get', payload: { scope: 'event-log', key: `inventory:${data.sku}` } })
  return log?.events || []
})

iii.registerTrigger({ type: 'http', function_id: 'query::catalog', config: { api_path: '/inventory', http_method: 'GET' } })
iii.registerTrigger({ type: 'http', function_id: 'query::sales-analytics', config: { api_path: '/inventory/analytics', http_method: 'GET' } })
iii.registerTrigger({ type: 'http', function_id: 'query::event-history', config: { api_path: '/inventory/history', http_method: 'GET' } })
