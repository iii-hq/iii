# Trigger Actions -- API Reference

Source: https://iii.dev/docs/how-to/trigger-actions

## TriggerRequest Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `function_id` | string | Yes | Target function (e.g., `'users::get'`) |
| `payload` | object | Yes | Input data for the function |
| `action` | TriggerAction | No | Execution mode (omit for synchronous) |
| `timeout_ms` | number | No | Max milliseconds to wait (sync only) |

## Invocation Modes

### Synchronous (default)

Omit the `action` parameter. Caller blocks until the function returns.

**Returns:** The function's return value directly.

**Use when:** You need the result, validation, reading data, immediate responses.

### Void

Use `TriggerAction.Void()`. Dispatches without waiting. No delivery guarantees.

**Returns:** `null` (TypeScript) / `None` (Python).

**Use when:** Analytics, logging, non-critical side effects.

### Enqueue

Use `TriggerAction.Enqueue({ queue: 'name' })`. Routes through a named queue with automatic retries.

**Returns:** `{ messageReceiptId: string }` for tracking.

**Use when:** Payments, order processing, anything that must complete reliably.

**Queue features:**
- Automatic retries via `max_retries` configuration
- Exponential backoff via `backoff_ms` setting
- FIFO ordering via `message_group_field`
- Dead letter queue on exhaustion

## Comparison Matrix

| Feature | Synchronous | Void | Enqueue |
|---------|------------|------|---------|
| Blocks caller | Yes | No | No |
| Returns value | Function result | null | messageReceiptId |
| Error handling | Propagates to caller | Silent | Retried, then DLQ |
| Retry support | None | None | Configurable |
| Ordering | Sequential | None | Optional FIFO |
| Concurrency control | N/A | N/A | Per-queue setting |

## TypeScript/Node.js

```typescript
import { init, TriggerAction } from 'iii-sdk'

const iii = init('ws://localhost:49134')

// Synchronous -- block for result
const user = await iii.trigger({
  function_id: 'users::get',
  payload: { id: 'usr_42' },
  timeoutMs: 5000,
})

// Void -- fire-and-forget
await iii.trigger({
  function_id: 'analytics::track-event',
  payload: { event: 'page_view', userId: 'usr_42' },
  action: TriggerAction.Void(),
})

// Enqueue -- durable with retries
const receipt = await iii.trigger({
  function_id: 'orders::process-payment',
  payload: { orderId: 'ord_789', amount: 149.99 },
  action: TriggerAction.Enqueue({ queue: 'payment' }),
})
// receipt.messageReceiptId available for tracking
```

## Python

```python
from iii import init, InitOptions, TriggerAction

iii = init('ws://localhost:49134', InitOptions(worker_name='my-worker'))

# Synchronous -- block for result
user = await iii.trigger({
    'function_id': 'users::get',
    'payload': {'id': 'usr_42'},
})

# Void -- fire-and-forget
await iii.trigger({
    'function_id': 'analytics::track-event',
    'payload': {'event': 'page_view', 'userId': 'usr_42'},
    'action': TriggerAction.Void(),
})

# Enqueue -- durable with retries
receipt = await iii.trigger({
    'function_id': 'orders::process-payment',
    'payload': {'orderId': 'ord_789', 'amount': 149.99},
    'action': TriggerAction.Enqueue(queue='payment'),
})
# receipt['messageReceiptId'] available for tracking
```

## Rust

```rust
use iii_sdk::{init, InitOptions, TriggerRequest, TriggerAction};
use serde_json::json;

let iii = init("ws://127.0.0.1:49134", InitOptions::default())?;

// Synchronous -- block for result
let user = iii.trigger(TriggerRequest {
    function_id: "users::get".into(),
    payload: json!({"id": "usr_42"}),
    action: None,
    timeout_ms: Some(5000),
}).await?;

// Void -- fire-and-forget
iii.trigger(TriggerRequest {
    function_id: "analytics::track-event".into(),
    payload: json!({"event": "page_view", "userId": "usr_42"}),
    action: Some(TriggerAction::Void),
    ..Default::default()
}).await?;

// Enqueue -- durable with retries
let receipt = iii.trigger(TriggerRequest {
    function_id: "orders::process-payment".into(),
    payload: json!({"orderId": "ord_789", "amount": 149.99}),
    action: Some(TriggerAction::Enqueue { queue: "payment".into() }),
    ..Default::default()
}).await?;
// receipt contains messageReceiptId
```

## Decision Guide

```
Need the result? ---------> Synchronous
  |
  No
  |
Must complete reliably? --> Enqueue (with queue name)
  |
  No
  |
Optional side effect? ----> Void
```
