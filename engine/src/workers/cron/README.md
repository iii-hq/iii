# iii-cron

Schedule functions to execute at specific times using cron expressions.

## Sample Configuration

```yaml
- name: iii-cron
  config:
    adapter:
      name: redis
      config:
        redis_url: ${REDIS_URL:redis://localhost:6379}
```

## Configuration

| Field | Type | Description |
|---|---|---|
| `adapter` | Adapter | Adapter for distributed locking. Defaults to `kv`. Use `redis` for multi-instance deployments. |

## Adapters

### kv

Built-in adapter using process-local locks. Suitable for single-instance deployments.

> **Warning:** When running multiple engine instances, the `kv` adapter does not provide reliable distributed locking — the same cron job may execute on every instance simultaneously. Use the `redis` adapter for multi-instance deployments.

```yaml
name: kv
config:
  lock_ttl_ms: 30000
  lock_index: cron_locks
```

| Field | Type | Description |
|---|---|---|
| `lock_ttl_ms` | integer | Duration in milliseconds for which a lock is held. Defaults to `30000`. |
| `lock_index` | string | Key namespace for lock entries. Defaults to `cron_locks`. |

### redis

Uses Redis for distributed locking to prevent duplicate job execution across multiple engine instances.

```yaml
name: redis
config:
  redis_url: ${REDIS_URL:redis://localhost:6379}
```

## Trigger Type: `cron`

| Config Field | Type | Description |
|---|---|---|
| `expression` | string | Required. Cron expression. Accepts 6-field (`second minute hour day month weekday`) or 7-field (with optional `year`) format. |
| `condition_function_id` | string | Function ID for conditional execution. If it returns `false`, the handler is skipped. |

### Cron Expression Format

```
* * * * * * [*]
│ │ │ │ │ │  │
│ │ │ │ │ │  └── Year (optional, * for any)
│ │ │ │ │ └──── Day of week (0–7, Sun=0 or 7)
│ │ │ │ └────── Month (1–12)
│ │ │ └──────── Day of month (1–31)
│ │ └────────── Hour (0–23)
│ └──────────── Minute (0–59)
└──────────── Second (0–59)
```

### Trigger Event Payload

| Field | Type | Description |
|---|---|---|
| `trigger` | string | Always `"cron"`. |
| `job_id` | string | The ID of the cron trigger that fired. |
| `scheduled_time` | string | The time the job was scheduled to run (RFC 3339). |
| `actual_time` | string | The actual time the job began executing (RFC 3339). |

### Sample Code

```typescript
const fn = iii.registerFunction(
  { id: 'jobs::cleanupOldData' },
  async (event) => {
    console.log('Running cleanup scheduled at:', event.scheduled_time)
    return {}
  },
)

iii.registerTrigger({
  type: 'cron',
  function_id: fn.id,
  config: { expression: '0 0 2 * * * *' },
})
```

## Common Cron Expressions

| Expression | Description |
|---|---|
| `0 * * * * *` | Every minute (6-field) |
| `0 0 * * * *` | Every hour (6-field) |
| `0 0 2 * * *` | Every day at 2 AM (6-field) |
| `0 0 0 * * * *` | Every day at midnight (7-field) |
| `0 0 0 * * 0 *` | Every Sunday at midnight |
| `0 */5 * * * * *` | Every 5 minutes |
| `0 0 9-17 * * 1-5 *` | Every hour from 9 AM to 5 PM, Monday to Friday |

## Distributed Execution

When running multiple III Engine instances, the once-only execution guarantee applies only with the `redis` adapter. Select it with `adapter.name: redis` and configure `adapter.config.redis_url` so all engine instances share the same Redis-backed lock store.

The default `kv` adapter uses process-local locks. In multi-instance deployments, each engine instance can acquire its own local lock and run the same cron job.

Cron handlers receive the trigger payload described above: `trigger`, `job_id`, `scheduled_time`, and `actual_time`.
