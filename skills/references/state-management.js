/**
 * Pattern: State Management
 * Comparable to: Redis, DynamoDB, Memcached
 *
 * Persistent key-value state scoped by namespace. Supports set, get,
 * list, delete, and atomic update operations (set, merge, append,
 * increment, decrement, remove). The merge op accepts a nested-segment
 * path for shallow-merging into auto-created intermediates.
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
iii.registerFunction('products::create', async (data) => {
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
iii.registerFunction('products::get', async (data) => {
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
iii.registerFunction('products::list-all', async () => {
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
iii.registerFunction('products::remove', async (data) => {
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
// state::update — Atomic ops over a record
// Payload: { scope, key, ops }
// ops: [{ type: 'set' | 'merge' | 'append' | 'increment' | 'decrement' | 'remove', path, value?, by? }]
// Use update instead of get-then-set for atomic partial changes.
// Returns { old_value, new_value, errors? } — failed ops surface
// structured entries in `errors` while later valid ops still apply.
// ---------------------------------------------------------------------------
iii.registerFunction('products::update-price', async (data) => {
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
// state::update with merge — Nested shallow-merge for per-session structured state
// `merge.path` accepts a string (first-level field) or an array of literal
// segments. The engine walks the segments, auto-creating each intermediate
// object. Sibling keys at every level are preserved.
// Each segment is a literal key — `["a.b"]` writes one key named "a.b",
// not a → b.
// ---------------------------------------------------------------------------
iii.registerFunction('transcripts::record-chunk', async (data) => {
  const { sessionId, chunk, author } = data
  const timestamp = Date.now()

  await iii.trigger({
    function_id: 'state::update',
    payload: {
      scope: 'audio::transcripts',
      key: sessionId,
      ops: [
        // Nested path: walks sessionId → "metadata", auto-creating
        // each intermediate object if it doesn't exist yet.
        { type: 'merge', path: [sessionId, 'metadata'], value: { author } },
        // First-level form (sugar for path: [sessionId]).
        { type: 'merge', path: sessionId, value: { [timestamp]: chunk } },
      ],
    },
  })

  return { sessionId, timestamp }
})

// ---------------------------------------------------------------------------
// HTTP triggers
// ---------------------------------------------------------------------------
iii.registerTrigger({
  type: 'http',
  function_id: 'products::create',
  config: { api_path: '/products', http_method: 'POST' },
})
iii.registerTrigger({
  type: 'http',
  function_id: 'products::get',
  config: { api_path: '/products/:id', http_method: 'GET' },
})
iii.registerTrigger({
  type: 'http',
  function_id: 'products::list-all',
  config: { api_path: '/products', http_method: 'GET' },
})
iii.registerTrigger({
  type: 'http',
  function_id: 'products::remove',
  config: { api_path: '/products/:id', http_method: 'DELETE' },
})
iii.registerTrigger({
  type: 'http',
  function_id: 'products::update-price',
  config: { api_path: '/products/:id/price', http_method: 'PUT' },
})
iii.registerTrigger({
  type: 'http',
  function_id: 'transcripts::record-chunk',
  config: { api_path: '/transcripts/:sessionId/chunks', http_method: 'POST' },
})
