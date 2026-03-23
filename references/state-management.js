/**
 * Pattern: State Management
 * Comparable to: Redis, DynamoDB, Memcached
 *
 * Persistent key-value state scoped by namespace. Supports set, get,
 * list, delete, and partial update operations.
 *
 * How-to references:
 *   - State management: https://iii.dev/docs/how-to/manage-state
 */

import { registerWorker, Logger, TriggerAction } from 'iii-sdk'

const iii = registerWorker(process.env.III_ENGINE_URL || 'ws://localhost:49134', {
  workerName: 'state-management',
})

// ---------------------------------------------------------------------------
// state::set — Store a value under a scoped key
// Payload: { scope, key, value }
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'products::create' }, async (data) => {
  const id = `prod-${Date.now()}`
  const product = {
    id,
    name: data.name,
    price: data.price,
    category: data.category,
    stock: data.stock || 0,
    created_at: new Date().toISOString(),
  }

  await iii.trigger({
    function_id: 'state::set',
    payload: { scope: 'products', key: id, value: product },
  })

  return product
})

// ---------------------------------------------------------------------------
// state::get — Retrieve a value by scope and key
// Payload: { scope, key }
// Returns null if the key does not exist — always guard for null.
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'products::get' }, async (data) => {
  const product = await iii.trigger({
    function_id: 'state::get',
    payload: { scope: 'products', key: data.id },
  })

  // Null guard — state::get returns null for missing keys
  if (!product) {
    return { error: 'Product not found', id: data.id }
  }

  return product
})

// ---------------------------------------------------------------------------
// state::list — Retrieve all values in a scope
// Payload: { scope }
// Returns an array of all stored values.
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'products::list-all' }, async () => {
  const products = await iii.trigger({
    function_id: 'state::list',
    payload: { scope: 'products' },
  })

  return { count: (products || []).length, products: products || [] }
})

// ---------------------------------------------------------------------------
// state::delete — Remove a key from a scope
// Payload: { scope, key }
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'products::remove' }, async (data) => {
  const existing = await iii.trigger({
    function_id: 'state::get',
    payload: { scope: 'products', key: data.id },
  })

  if (!existing) {
    return { error: 'Product not found', id: data.id }
  }

  await iii.trigger({
    function_id: 'state::delete',
    payload: { scope: 'products', key: data.id },
  })

  return { deleted: data.id }
})

// ---------------------------------------------------------------------------
// state::update — Partial merge using ops array
// Payload: { scope, key, ops }
// ops: [{ type: 'set', path, value }]
// Use update instead of get-then-set for atomic partial changes.
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'products::update-price' }, async (data) => {
  const existing = await iii.trigger({
    function_id: 'state::get',
    payload: { scope: 'products', key: data.id },
  })

  if (!existing) {
    return { error: 'Product not found', id: data.id }
  }

  await iii.trigger({
    function_id: 'state::update',
    payload: {
      scope: 'products',
      key: data.id,
      ops: [
        { type: 'set', path: 'price', value: data.newPrice },
        { type: 'set', path: 'updated_at', value: new Date().toISOString() },
      ],
    },
  })

  return { id: data.id, price: data.newPrice }
})

// ---------------------------------------------------------------------------
// Combining operations — inventory adjustment with update
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'products::adjust-stock' }, async (data) => {
  const logger = new Logger()

  const product = await iii.trigger({
    function_id: 'state::get',
    payload: { scope: 'products', key: data.id },
  })

  if (!product) {
    return { error: 'Product not found', id: data.id }
  }

  const newStock = product.stock + data.adjustment

  if (newStock < 0) {
    return { error: 'Insufficient stock', current: product.stock, requested: data.adjustment }
  }

  await iii.trigger({
    function_id: 'state::update',
    payload: {
      scope: 'products',
      key: data.id,
      ops: [
        { type: 'set', path: 'stock', value: newStock },
        { type: 'set', path: 'last_stock_change', value: new Date().toISOString() },
      ],
    },
  })

  logger.info('Stock adjusted', { id: data.id, from: product.stock, to: newStock })
  return { id: data.id, previousStock: product.stock, newStock }
})

// ---------------------------------------------------------------------------
// HTTP triggers
// ---------------------------------------------------------------------------
iii.registerTrigger({ type: 'http', function_id: 'products::create', config: { api_path: '/products', http_method: 'POST' } })
iii.registerTrigger({ type: 'http', function_id: 'products::get', config: { api_path: '/products/:id', http_method: 'GET' } })
iii.registerTrigger({ type: 'http', function_id: 'products::list-all', config: { api_path: '/products', http_method: 'GET' } })
iii.registerTrigger({ type: 'http', function_id: 'products::remove', config: { api_path: '/products/:id', http_method: 'DELETE' } })
iii.registerTrigger({ type: 'http', function_id: 'products::update-price', config: { api_path: '/products/:id/price', http_method: 'PUT' } })
iii.registerTrigger({ type: 'http', function_id: 'products::adjust-stock', config: { api_path: '/products/:id/stock', http_method: 'POST' } })
