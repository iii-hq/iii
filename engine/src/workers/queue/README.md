# iii-queue

Asynchronous job processing with named queues, retries, and dead-letter support.

Supports two modes:

- **Topic-based queues** — register a consumer per topic, emit events via `iii::durable::publish`. Fan-out: every distinct function subscribed to a topic receives a copy of each message.
- **Named queues** — define queues in config, then enqueue function calls via `TriggerAction.Enqueue`. No trigger registration needed.

## Sample Configuration

```yaml
- name: iii-queue
  config:
    queue_configs:
      default:
        max_retries: 5
        concurrency: 5
        type: standard
      payment:
        max_retries: 10
        concurrency: 2
        type: fifo
        message_group_field: transaction_id
    adapter:
      name: builtin
      config:
        store_method: file_based
        file_path: ./data/queue_store
```

## Configuration

| Field | Type | Description |
|---|---|---|
| `queue_configs` | map[string, FunctionQueueConfig] | Map of named queue configurations. Each key is the queue name. |
| `adapter` | Adapter | Transport adapter. Defaults to `builtin`. |

### Queue Configuration (`queue_configs` entries)

| Field | Type | Description |
|---|---|---|
| `max_retries` | u32 | Maximum delivery attempts before routing to DLQ. Defaults to `3`. |
| `concurrency` | u32 | Maximum jobs processed simultaneously. Defaults to `10`. FIFO queues override this to `prefetch=1`. |
| `type` | string | `standard` (concurrent) or `fifo` (ordered within a message group). |
| `message_group_field` | string | Required for `fifo`. JSON field whose value determines the ordering group. |
| `backoff_ms` | u64 | Base retry backoff in milliseconds. Exponential: `backoff_ms × 2^(attempt−1)`. Defaults to `1000`. |
| `poll_interval_ms` | u64 | Worker poll interval in milliseconds. Defaults to `100`. |

## Queue Modes

### When to use which

| | Topic-based | Named queues |
|---|---|---|
| **Producer** | `trigger({ function_id: 'iii::durable::publish', payload: { topic, data } })` | `trigger({ function_id, payload, action: TriggerAction.Enqueue({ queue }) })` |
| **Consumer** | `registerTrigger({ type: 'durable:subscriber', config: { topic } })` | No registration — function is the target |
| **Delivery** | Fan-out: each subscribed function gets every message; replicas compete | Single target function per enqueue call |
| **Config** | Optional `queue_config` on trigger | `queue_configs` in `iii-config.yaml` |
| **Use case** | Durable pub/sub with retries and fan-out | Direct function invocation with retries, FIFO, DLQ |

### Standard vs FIFO Queues

| Dimension | Standard | FIFO |
|-----------|----------|------|
| **Processing model** | Up to `concurrency` jobs in parallel | One job at a time (prefetch=1) |
| **Ordering** | No guarantees | Strictly ordered within a message group |
| **`message_group_field`** | Not required | Required — must be present and non-null in every payload |
| **Throughput** | High | Lower — trades throughput for ordering |
| **Use cases** | Email sends, image processing, notifications | Payments, ledger entries, state machines |
| **Retries** | Retried independently, other jobs continue | Retried inline — blocks the queue until success or DLQ |

## Adapters

### builtin

Built-in in-process queue. No external dependencies. Suitable for single-instance deployments.

```yaml
name: builtin
config:
  store_method: file_based   # in_memory | file_based
  file_path: ./data/queue_store
```

### redis

Uses Redis Pub/Sub for topic-based queues. Supports multi-instance deployments.

> **Note:** The Redis adapter supports publishing to named queues but does not implement named queue consumption, retries, or dead-letter queues. For full named queue support in multi-instance deployments, use RabbitMQ.

```yaml
name: redis
config:
  redis_url: ${REDIS_URL:redis://localhost:6379}
```

### rabbitmq

Uses RabbitMQ for durable delivery, retries, and dead-letter queues across multiple engine instances.

```yaml
name: rabbitmq
config:
  amqp_url: ${RABBITMQ_URL:amqp://localhost:5672}
```

### Adapter Comparison

| | builtin | rabbitmq | redis |
|---|---|---|---|
| **Retries** | Yes | Yes | No |
| **Dead-letter queue** | Yes | Yes | No |
| **FIFO ordering** | Yes | Yes | No |
| **Named queue consumption** | Yes | Yes | No (publish only) |
| **Topic-based pub/sub** | Yes | Yes | Yes |
| **Multi-instance** | No | Yes | Yes |
| **External dependency** | None | RabbitMQ | Redis |

| Scenario | Recommended Adapter |
|----------|-------------------|
| Local development | `builtin` (`in_memory`) |
| Single-instance production | `builtin` (`file_based`) |
| Multi-instance production | `rabbitmq` |

## Builtin Functions

### `iii::durable::publish`

Publishes a message to a topic-based queue. Fanned out to every distinct subscribed function.

| Field | Type | Description |
|-------|------|-------------|
| `topic` | string | The topic to publish to (required). |
| `data` | any | The payload delivered to each subscribed function. |

### `iii::queue::redrive`

Moves all messages from a named queue's dead-letter queue back to the main queue.

| Field | Type | Description |
|-------|------|-------------|
| `queue` | string | The named queue whose DLQ should be redriven. |

Returns: `queue` (string), `redriven` (number)

```bash
iii trigger \
  --function-id='iii::queue::redrive' \
  --payload='{"queue": "payment"}'
```
