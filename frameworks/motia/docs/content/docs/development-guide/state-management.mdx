---
title: State Management
description: Persistent Key-Value storage that works across Triggers, Steps, and Functions
---

<video controls className="mb-8 w-full rounded-xl" poster="https://assets.motia.dev/images/gifs/v1/6-motia-state.gif">
  <source src="https://assets.motia.dev/videos/mp4/site/v1/6-motia-state.mp4" type="video/mp4" />
</video>

State is persistent key-value storage that works across all your Triggers, Steps, and Functions. Set data in one Trigger, read it in another. Works across TypeScript, Python, and JavaScript.

## How It Works

State organizes data into **groups**. Each group can hold multiple items with unique keys.

Think of it like folders and files:
- **groupId** = A folder name (like `orders`, `users`, `cache`)
- **key** = A file name inside that folder
- **value** = The actual data

<Tabs items={['TypeScript', 'Python', 'JavaScript']}>
<Tab value='TypeScript'>

```typescript
import { type Handlers, type StepConfig, stateManager } from 'motia'

export const config = {
  name: 'MyStep',
  description: 'Demonstrates state usage',
  triggers: [
    { type: 'queue', topic: 'my-topic' },
  ],
  flows: ['my-flow'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (input) => {
  // Store an item in a group (returns { new_value, old_value })
  const result = await stateManager.set('orders', 'order-123', {
    id: 'order-123',
    status: 'pending',
    total: 99.99
  })

  // Get a specific item
  const order = await stateManager.get('orders', 'order-123')

  // Get all items in a group
  const allOrders = await stateManager.list('orders')

  // Delete a specific item
  await stateManager.delete('orders', 'order-123')

  // Clear entire group
  await stateManager.clear('orders')
}
```

</Tab>
<Tab value='Python'>

```python
from motia import state_manager

async def handler(input):
    # Store an item in a group (returns { new_value, old_value })
    result = await state_manager.set("orders", "order-123", {
        "id": "order-123",
        "status": "pending",
        "total": 99.99
    })

    # Get a specific item
    order = await state_manager.get("orders", "order-123")

    # Get all items in a group
    all_orders = await state_manager.list("orders")

    # Delete a specific item
    await state_manager.delete("orders", "order-123")

    # Clear entire group
    await state_manager.clear("orders")
  ```

  </Tab>
<Tab value='JavaScript'>

  ```javascript
import { stateManager } from 'motia'

export const config = {
  name: 'MyStep',
  description: 'Demonstrates state usage',
  triggers: [
    { type: 'queue', topic: 'my-topic' },
  ],
  flows: ['my-flow'],
}

export const handler = async (input) => {
  // Store an item in a group (returns { new_value, old_value })
  const result = await stateManager.set('orders', 'order-123', {
    id: 'order-123',
    status: 'pending',
    total: 99.99
  })

  // Get a specific item
  const order = await stateManager.get('orders', 'order-123')

  // Get all items in a group
  const allOrders = await stateManager.list('orders')

  // Delete a specific item
  await stateManager.delete('orders', 'order-123')

  // Clear entire group
  await stateManager.clear('orders')
  }
  ```

  </Tab>
</Tabs>

---

## State Methods

| Method | What it does |
|--------|--------------|
| `stateManager.set(groupId, key, value)` | Store an item in a group. Returns `StreamSetResult` with `new_value` and `old_value` |
| `stateManager.get(groupId, key)` | Get a specific item (returns `null` if not found) |
| `stateManager.list(groupId)` | Get all items in a group as an array |
| `stateManager.delete(groupId, key)` | Remove a specific item |
| `stateManager.clear(groupId)` | Remove all items in a group |
| `stateManager.update(groupId, key, ops)` | Atomic update with `UpdateOp[]` |

---

## Atomic Updates

The `update()` method performs atomic operations on state data, eliminating race conditions from manual get-then-set patterns.

<Callout type="info">
  Use `update()` instead of getting a value, modifying it, and setting it back. Atomic updates prevent data loss when multiple Steps modify the same state concurrently.
</Callout>

### TypeScript/JavaScript

```typescript
import { stateManager } from 'motia'

await stateManager.update('orders', orderId, [
  { type: 'increment', path: 'completedSteps', by: 1 },
  { type: 'set', path: 'status', value: 'shipped' },
  { type: 'decrement', path: 'retries', by: 1 },
])
```

### Python

```python
from motia import state_manager

await state_manager.update("orders", order_id, [
    {"type": "increment", "path": "completedSteps", "by": 1},
    {"type": "set", "path": "status", "value": "shipped"},
    {"type": "decrement", "path": "retries", "by": 1},
])
```

### UpdateOp Types

| Type | Fields | Description |
|---|---|---|
| `set` | `path`, `value` | Set a field to a value (overwrite) |
| `merge` | `path` (optional), `value` | Merge an object into the existing value |
| `increment` | `path`, `by` | Increment a numeric field |
| `decrement` | `path`, `by` | Decrement a numeric field |
| `remove` | `path` | Remove a field entirely |

`update()` returns `{ new_value, old_value }` just like `set()`.

[Learn more about Atomic Updates](/docs/advanced-features/atomic-updates)

---

## Real-World Example

Let's build an order processing workflow that uses state across multiple Steps.

**Step 1 - API receives order:**

<Tabs items={['TypeScript', 'Python', 'JavaScript']}>
<Tab value='TypeScript'>

```typescript
import { enqueue, type Handlers, logger, type StepConfig, stateManager } from 'motia'

export const config = {
  name: 'CreateOrder',
  description: 'Receive and store a new order',
  triggers: [
    { type: 'http', path: '/orders', method: 'POST' },
  ],
  enqueues: ['order.created'],
  flows: ['order-processing'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async ({ request }) => {
  const orderId = crypto.randomUUID()

  const order = {
    id: orderId,
    items: request.body.items,
    total: request.body.total,
    status: 'pending',
    createdAt: new Date().toISOString()
  }

  // Store in state
  await stateManager.set('orders', orderId, order)

  logger.info('Order created', { orderId })

  // Trigger processing
  await enqueue({
    topic: 'order.created',
    data: { orderId }
  })

  return { status: 201, body: order }
}
```

  </Tab>
<Tab value='Python'>

```python
import uuid
from datetime import datetime
from typing import Any
from motia import ApiRequest, ApiResponse, enqueue, logger, state_manager

async def handler(request: ApiRequest[Any]) -> ApiResponse[Any]:
    order_id = str(uuid.uuid4())

    order = {
        "id": order_id,
        "items": request.body.get("items"),
        "total": request.body.get("total"),
        "status": "pending",
        "created_at": datetime.now().isoformat()
    }

    # Store in state
    await state_manager.set("orders", order_id, order)

    logger.info("Order created", {"orderId": order_id})

    # Trigger processing
    await enqueue({
        "topic": "order.created",
        "data": {"orderId": order_id}
    })

    return ApiResponse(status=201, body=order)
  ```

  </Tab>
<Tab value='JavaScript'>

  ```javascript
import { enqueue, logger, stateManager } from 'motia'

export const handler = async ({ request }) => {
  const orderId = crypto.randomUUID()

  const order = {
    id: orderId,
    items: request.body.items,
    total: request.body.total,
    status: 'pending',
    createdAt: new Date().toISOString()
  }

  // Store in state
  await stateManager.set('orders', orderId, order)

  logger.info('Order created', { orderId })

  // Trigger processing
  await enqueue({
    topic: 'order.created',
    data: { orderId }
  })

  return { status: 201, body: order }
}
  ```

  </Tab>
</Tabs>

**Step 2 - Process payment:**

<Tabs items={['TypeScript', 'Python', 'JavaScript']}>
<Tab value='TypeScript'>

  ```typescript
import { enqueue, type Handlers, logger, type StepConfig, stateManager } from 'motia'

export const config = {
  name: 'ProcessPayment',
  description: 'Process payment for an order',
  triggers: [
    { type: 'queue', topic: 'order.created' },
  ],
  enqueues: ['payment.completed'],
  flows: ['order-processing'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (input) => {
  const { orderId } = input

  const updatedOrder = await stateManager.update('orders', orderId, [
    { type: 'set', path: 'status', value: 'paid' },
  ])
  if (!updatedOrder) {
    throw new Error(`Order ${orderId} not found`)
  }

  logger.info('Payment processed', { orderId })

  await enqueue({
    topic: 'payment.completed',
    data: { orderId }
  })
}
  ```

  </Tab>
<Tab value='Python'>

```python
from motia import enqueue, logger, state_manager

async def handler(input):
    order_id = input.get("orderId")

    updated_order = await state_manager.update("orders", order_id, [
        {"type": "set", "path": "status", "value": "paid"}
    ])
    if not updated_order:
        raise Exception(f"Order {order_id} not found")

    logger.info("Payment processed", {"orderId": order_id})

    await enqueue({
        "topic": "payment.completed",
        "data": {"orderId": order_id}
    })
```

</Tab>
<Tab value='JavaScript'>

```javascript
import { enqueue, logger, stateManager } from 'motia'

export const handler = async (input) => {
  const { orderId } = input

  const updatedOrder = await stateManager.update('orders', orderId, [
    { type: 'set', path: 'status', value: 'paid' },
  ])
  if (!updatedOrder) {
    throw new Error(`Order ${orderId} not found`)
  }

  logger.info('Payment processed', { orderId })

  await enqueue({
    topic: 'payment.completed',
    data: { orderId }
  })
}
  ```

  </Tab>
</Tabs>

**Step 3 - View all orders (Cron job):**

<Tabs items={['TypeScript', 'Python', 'JavaScript']}>
<Tab value='TypeScript'>

  ```typescript
import { type Handlers, logger, type StepConfig, stateManager, cron } from 'motia'

export const config = {
  name: 'DailyReport',
  description: 'Generate daily order report',
  triggers: [
    cron('0 0 * * *'),
  ],
  enqueues: [],
  flows: ['order-processing'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (input) => {
  // Get all orders
  const allOrders = await stateManager.list<Order>('orders')

  const pending = allOrders.filter(o => o.status === 'pending')
  const paid = allOrders.filter(o => o.status === 'paid')

  logger.info('Daily order report', {
    total: allOrders.length,
    pending: pending.length,
    paid: paid.length
  })
}
```

  </Tab>
<Tab value='Python'>

```python
from motia import logger, state_manager

async def handler():
    # Get all orders
    all_orders = await state_manager.list("orders")

    pending = [o for o in all_orders if o.get("status") == "pending"]
    paid = [o for o in all_orders if o.get("status") == "paid"]

    logger.info("Daily order report", {
        "total": len(all_orders),
        "pending": len(pending),
        "paid": len(paid)
    })
```

</Tab>
<Tab value='JavaScript'>

  ```javascript
import { logger, stateManager } from 'motia'

export const handler = async (input) => {
  // Get all orders
  const allOrders = await stateManager.list('orders')

  const pending = allOrders.filter(o => o.status === 'pending')
  const paid = allOrders.filter(o => o.status === 'paid')

  logger.info('Daily order report', {
    total: allOrders.length,
    pending: pending.length,
    paid: paid.length
  })
}
```

  </Tab>
</Tabs>

---

## When to Use State

**Good use cases:**
- **Temporary workflow data** - Data that's only needed during a flow execution
- **API response caching** - Cache expensive API calls that don't change often
- **Sharing data between Steps** - Pass data between Steps without enqueuing it in events
- **Building up results** - Accumulate data across multiple Steps

**Better alternatives:**
- **Persistent user data** - Use a database like Postgres or MongoDB
- **File storage** - Use S3 or similar for images, PDFs, documents
- **Real-time updates** - Use Motia Streams for live data to clients
- **Large datasets** - Use a proper database, not state

---

## Inspecting State in the iii Console

The iii development console lets you browse, inspect, and manage state groups and individual entries in real-time:

![State inspector in the iii Console](/console/states-detail.png)

---

## Remember

- Organize data using **groupId** (like `orders`, `users`, `cache`)
- Each item needs a unique **key** within its groupId
- Use `stateManager.list(groupId)` to retrieve all items in a group
- `stateManager.set()` returns `{ new_value, old_value }` (StreamSetResult)
- Use `stateManager.update()` for atomic operations like increment/decrement
- State works the same across TypeScript, Python, and JavaScript
- Clean up state when you're done with it
- Use databases for permanent data, state for temporary workflow data

---

## State Triggers

Steps can react to state changes automatically using state triggers. This enables powerful reactive patterns — for example, triggering a step when a parallel merge completes, without polling or manual coordination.

```typescript
export const config = {
  name: 'OnTaskComplete',
  triggers: [
    {
      type: 'state',
      condition: (input) => {
        if (!input.new_value) return false
        return input.group_id === 'tasks' && input.new_value.completedSteps === input.new_value.totalSteps
      },
    },
  ],
  flows: ['task-flow'],
} as const satisfies StepConfig
```

[Learn more about State Triggers](/docs/advanced-features/reactive-triggers)
