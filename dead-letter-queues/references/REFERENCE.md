# Dead Letter Queues -- API Reference

Source: https://iii.dev/docs/how-to/dead-letter-queues

## How Jobs Reach the DLQ

1. A queued job fails during execution
2. The engine retries with exponential backoff: `backoff_ms * 2^(attempt_number)`
3. After exhausting `max_retries`, the job is routed to the DLQ automatically
4. The DLQ entry contains the original payload, last error, timestamp, and job metadata

## RabbitMQ Queue Topology

For a queue named `payment`, the engine creates:

| Resource | Name |
|----------|------|
| Main queue | `iii.__fn_queue::payment.queue` |
| Retry queue | `iii.__fn_queue::payment::retry.queue` |
| DLQ queue | `iii.__fn_queue::payment::dlq.queue` |
| DLQ exchange | `iii.__fn_queue::payment::dlq` |

The main queue configures `x-dead-letter-exchange` pointing to the DLQ exchange for automatic routing.

## Inspecting DLQ Jobs

### RabbitMQ Management UI

1. Navigate to `http://localhost:15672`
2. Find the DLQ queue: `iii.__fn_queue::{name}::dlq.queue`
3. Use "Get messages" with Ack mode set to `Nack message requeue true` to inspect without consuming

### rabbitmqadmin CLI

```bash
rabbitmqadmin get queue="iii.__fn_queue::payment::dlq.queue" count=10 ackmode=ack_requeue_true
```

### Builtin Adapter (No RabbitMQ)

Monitor engine logs for warnings containing "Job exhausted, moved to DLQ" with queue name, job ID, and attempt count.

## Redriving Jobs

### SDK (TypeScript)

```typescript
import { init } from 'iii-sdk'

const iii = init('ws://localhost:49134')

const result = await iii.trigger({
  function_id: 'iii::queue::redrive',
  payload: { queue: 'payment' },
})
// result: { queue: 'payment', redriven: 12 }
```

### SDK (Python)

```python
from iii import init, InitOptions

iii = init('ws://localhost:49134', InitOptions(worker_name='ops-worker'))

result = await iii.trigger({
    'function_id': 'iii::queue::redrive',
    'payload': {'queue': 'payment'},
})
# result: {'queue': 'payment', 'redriven': 12}
```

### SDK (Rust)

```rust
use iii_sdk::{init, InitOptions, TriggerRequest};
use serde_json::json;

let iii = init("ws://127.0.0.1:49134", InitOptions::default())?;

let result = iii.trigger(TriggerRequest {
    function_id: "iii::queue::redrive".into(),
    payload: json!({"queue": "payment"}),
    ..Default::default()
}).await?;
// result: {"queue": "payment", "redriven": 12}
```

### CLI

```bash
iii trigger \
  --function-id='iii::queue::redrive' \
  --payload='{"queue": "payment"}'
```

## Redrive Return Type

```json
{
  "queue": "payment",
  "redriven": 12
}
```

| Field | Type | Description |
|-------|------|-------------|
| `queue` | string | The queue name that was redriven |
| `redriven` | number | Count of jobs moved back to the main queue |

Redriving resets attempt counters to zero, providing a fresh retry cycle for each message.

## Best Practices

1. **Monitor DLQ depth** -- set alerts on message counts to detect systemic issues early
2. **Investigate before redriving** -- understand why jobs failed before pushing them back
3. **Deploy fixes first** -- verify the fix works against new messages before redriving old ones
4. **Discard stale messages** -- remove irrelevant or outdated jobs rather than redriving them
5. **Redrive in batches** -- avoid overwhelming downstream systems by processing in controlled batches
