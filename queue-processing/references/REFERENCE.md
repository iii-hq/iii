# Queue Processing -- API Reference

Source: https://iii.dev/docs/how-to/use-queues

## Module Configuration

Define queues in `iii-config.yaml` under a queue module:

```yaml
modules:
  - class: modules::queue::QueueModule
    config:
      adapter:
        class: modules::queue::adapters::BuiltinQueueAdapter
        config:
          store_method: file_based  # or in_memory
      queue_configs:
        emails:
          max_retries: 5
          concurrency: 10
          type: standard
          backoff_ms: 1000
          poll_interval_ms: 100
        payments:
          max_retries: 3
          concurrency: 1
          type: fifo
          message_group_field: account_id
          backoff_ms: 2000
```

## Configuration Parameters

| Field | Type | Default | Purpose |
|-------|------|---------|---------|
| `max_retries` | u32 | 3 | Total delivery attempts before DLQ |
| `concurrency` | u32 | 10 | Max simultaneous workers (standard only) |
| `type` | string | "standard" | Processing mode: `standard` or `fifo` |
| `message_group_field` | string | -- | FIFO ordering key (required for FIFO) |
| `backoff_ms` | u64 | 1000 | Base retry delay in milliseconds |
| `poll_interval_ms` | u64 | 100 | Worker polling interval |

## Queue Types

| Aspect | Standard | FIFO |
|--------|----------|------|
| Processing | Up to `concurrency` jobs in parallel | Sequential within message groups |
| Ordering | No guarantees | Strictly ordered per group |
| `message_group_field` | Not required | Required, non-null in payload |
| Throughput | High | Lower (trades speed for ordering) |
| Use cases | Email, images, notifications | Payments, ledgers, state changes |

## Retry Backoff Formula

```
delay = backoff_ms * 2^(attempt - 1)
```

Example with `backoff_ms: 1000`: 1s -> 2s -> 4s -> 8s -> 16s

## Enqueue a Job

### TypeScript/Node.js

```typescript
import { init, TriggerAction } from 'iii-sdk'

const iii = init('ws://localhost:49134')

const receipt = await iii.trigger({
  function_id: 'orders::process-payment',
  payload: { orderId: 'ord_789', amount: 149.99, currency: 'USD' },
  action: TriggerAction.Enqueue({ queue: 'payments' }),
})
console.log(receipt.messageReceiptId)
```

### Python

```python
import os
from iii import init, TriggerAction

iii = init(os.environ.get("III_URL", "ws://localhost:49134"))

receipt = iii.trigger({
    "function_id": "orders::process-payment",
    "payload": {"orderId": "ord_789", "amount": 149.99, "currency": "USD"},
    "action": TriggerAction.Enqueue(queue="payments")
})
print(receipt["messageReceiptId"])
```

### Rust

```rust
use iii_sdk::{init, InitOptions, TriggerAction};
use serde_json::json;

let iii = init("ws://127.0.0.1:49134", InitOptions::default())?;

let receipt = iii.trigger(json!({
    "function_id": "orders::process-payment",
    "payload": { "orderId": "ord_789", "amount": 149.99, "currency": "USD" },
    "action": TriggerAction::Enqueue { queue: "payments".into() }
}))?;
```

## Register the Job Processor

Target functions receive the enqueued payload directly. No queue-specific handler changes needed.

```typescript
iii.registerFunction({ id: 'orders::process-payment' }, async (payload) => {
  // payload = { orderId, amount, currency }
  // Works identically whether invoked directly or via queue
  const result = await processPayment(payload.orderId, payload.amount)
  return { success: true, transactionId: result.id }
})
```

## FIFO Queue Requirements

Payload must include a non-null value for the configured `message_group_field`:

```typescript
// queue config: { type: 'fifo', message_group_field: 'account_id' }
await iii.trigger({
  function_id: 'ledger::record-transaction',
  payload: {
    account_id: 'acct_456',  // must match message_group_field, must not be null
    amount: 49.99,
    type: 'credit',
  },
  action: TriggerAction.Enqueue({ queue: 'ledger' }),
})
```

## Error Handling

Enqueue operations fail synchronously when:
- Queue name does not exist in configuration
- FIFO queue payload is missing the required `message_group_field`
- Field value is null (FIFO only)

```typescript
try {
  await iii.trigger({
    function_id: 'orders::process',
    payload: data,
    action: TriggerAction.Enqueue({ queue: 'nonexistent' }),
  })
} catch (err) {
  if (err.enqueue_error) {
    console.error('Rejected:', err.enqueue_error)
  }
}
```

## Adapters

### BuiltinQueueAdapter

- `in_memory` -- development only, data lost on restart
- `file_based` -- single instance, persists to `./data/queue_store`

### RabbitMQAdapter

For multi-instance distributed deployments:

```yaml
adapter:
  class: modules::queue::adapters::RabbitMQAdapter
  config:
    url: amqp://guest:guest@localhost:5672
```

Queue naming convention:
- Main: `iii.__fn_queue::queue_name`
- Retry: `iii.__fn_queue::queue_name::retry.queue`
- DLQ: `iii.__fn_queue::queue_name::dlq.queue`

## Complete Example: HTTP + Queue Pipeline

```typescript
import { init, TriggerAction, Logger } from 'iii-sdk'

const iii = init('ws://localhost:49134')
const logger = new Logger()

// HTTP endpoint accepts order, enqueues for processing
iii.registerFunction({ id: 'orders::submit' }, async (req) => {
  const order = { id: crypto.randomUUID(), ...req.body }
  logger.info('Order received', { orderId: order.id })

  await iii.trigger({
    function_id: 'orders::process',
    payload: order,
    action: TriggerAction.Enqueue({ queue: 'orders' }),
  })

  return { status_code: 202, body: { orderId: order.id, status: 'queued' } }
})

iii.registerTrigger({
  type: 'http',
  function_id: 'orders::submit',
  config: { api_path: '/orders', http_method: 'POST' },
})

// Queue processor handles the order
iii.registerFunction({ id: 'orders::process' }, async (payload) => {
  logger.info('Processing order', { orderId: payload.id })
  // ... processing logic with automatic retries on failure
  return { processed: true }
})
```
