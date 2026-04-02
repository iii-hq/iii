---
title: Atomic Updates (UpdateOp)
description: Perform atomic field-level updates on state and stream data without race conditions
---

The `UpdateOp` system lets you perform atomic field-level updates on both state and stream data. Instead of reading an entire object, modifying it, and writing it back (which creates race conditions when multiple Steps access the same data), you describe the operations and they are applied atomically.

## The Problem

The traditional get-then-set pattern is not safe when multiple Steps run concurrently:

```typescript
const order = await state.get('orders', orderId)
order.completedSteps += 1
order.status = 'progress'
await state.set('orders', orderId, order)
```

If two Steps read the same order simultaneously, one update will be lost.

## The Solution

Use `update()` with `UpdateOp[]` for atomic operations:

```typescript
await state.update('orders', orderId, [
  { type: 'increment', path: 'completedSteps', by: 1 },
  { type: 'set', path: 'status', value: 'progress' },
])
```

All operations in the array are applied atomically — no data loss, no race conditions.

---

## UpdateOp Types

| Type | Fields | Description |
|---|---|---|
| `set` | `path`, `value` | Set a field to a specific value (overwrite) |
| `merge` | `path` (optional), `value` | Merge an object into the existing value (object fields only) |
| `increment` | `path`, `by` | Increment a numeric field by the given amount |
| `decrement` | `path`, `by` | Decrement a numeric field by the given amount |
| `remove` | `path` | Remove a field entirely |

---

## Usage in State

```typescript
import { stateManager } from 'motia'

await stateManager.update<Order>('orders', orderId, [
  { type: 'increment', path: 'completedSteps', by: 1 },
  { type: 'set', path: 'status', value: 'shipped' },
  { type: 'decrement', path: 'retries', by: 1 },
  { type: 'remove', path: 'tempData' },
])
```

Returns `{ new_value, old_value }` — the same return type as `stateManager.set()`.

### Python

```python
from motia import state_manager

await state_manager.update("orders", order_id, [
    {"type": "increment", "path": "completedSteps", "by": 1},
    {"type": "set", "path": "status", "value": "shipped"},
    {"type": "decrement", "path": "retries", "by": 1},
    {"type": "remove", "path": "tempData"},
])
```

---

## Usage in Streams

The same `UpdateOp` types work on stream data:

```typescript
import { deploymentStream } from './deployment.stream'

export const handler: Handlers<typeof config> = async (input) => {
  await deploymentStream.update('data', deploymentId, [
    { type: 'increment', path: 'completedSteps', by: 1 },
    { type: 'set', path: 'status', value: 'progress' },
  ])
}
```

Stream updates are also atomic and trigger stream events that connected clients receive in real-time.

---

## Merge Operation

The `merge` operation performs a shallow merge of an object into the existing value:

```typescript
import { stateManager } from 'motia'

await stateManager.update('users', userId, [
  {
    type: 'merge',
    path: 'preferences',
    value: { theme: 'dark', language: 'en' },
  },
])
```

If `path` is omitted, the merge is applied to the root object:

```typescript
import { stateManager } from 'motia'

await stateManager.update('users', userId, [
  {
    type: 'merge',
    value: { lastLogin: new Date().toISOString(), loginCount: 5 },
  },
])
```

---

## Common Patterns

### Counter Tracking

```typescript
import { stateManager } from 'motia'

await stateManager.update('metrics', 'api-calls', [
  { type: 'increment', path: 'total', by: 1 },
  { type: 'increment', path: `endpoints.${endpoint}`, by: 1 },
  { type: 'set', path: 'lastCall', value: new Date().toISOString() },
])
```

### Status Transitions

```typescript
import { stateManager } from 'motia'

await stateManager.update('orders', orderId, [
  { type: 'set', path: 'status', value: 'completed' },
  { type: 'set', path: 'completedAt', value: new Date().toISOString() },
  { type: 'remove', path: 'processingData' },
])
```

### Parallel Step Completion

```typescript
import { stateManager } from 'motia'

await stateManager.update('tasks', taskId, [
  { type: 'increment', path: 'completedSteps', by: 1 },
  { type: 'merge', path: 'results', value: { [stepName]: result } },
])
```

Combined with [state triggers](/docs/advanced-features/reactive-triggers), this pattern enables powerful parallel-then-merge workflows without polling.
