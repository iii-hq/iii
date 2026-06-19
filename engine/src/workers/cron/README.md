# iii-cron

Schedule functions to execute at specific times using cron expressions.

## Install

```bash
iii worker add iii-cron
```

Resolves from the worker registry at [workers.iii.dev](https://workers.iii.dev/).

## Skills

Install the `iii-cron` agent skill for Claude Code, Cursor, and 30+ other agents:

```bash
npx skills add iii-hq/iii --full-depth --skill iii-cron
```

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

## Runtime configuration (hot reload)

`iii-cron` registers its configuration with the builtin `configuration` worker
under the id **`iii-cron`**, so the adapter above can be read and changed at
runtime (e.g. `configuration::set { id: "iii-cron", value: { ... } }`) without
restarting the engine. The config.yaml block is the **seed** used on first boot
only; afterwards the configuration entry is the runtime source of truth and a
runtime edit survives engine restarts. Values are validated against the schema
at set time, and `${VAR:default}` placeholders are expanded on read.

A change to `adapter` is a **full hot-swap**, all without a restart: the new lock
backend is built first (the swap is **gated** — a value that fails to build keeps
the previous backend, config, and jobs), then the old backend is shut down
(stopping its tasks and releasing its locks), and finally every live cron job is
re-registered onto the new backend **best-effort** (a single job that fails to
re-register is logged while the rest move over). Shutting the old backend down
before re-registering avoids a window where a job runs on both backends at once;
the trade-off is a brief scheduling gap on this instance — a job whose fire time
lands in the swap window is skipped, not run twice. Across a multi-instance
deployment the old and new lock backends still cannot coordinate while instances
sit on different backends mid-migration, so a job could run on more than one
instance — prefer a quiet moment to repoint the adapter.

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
| `lock_ttl_ms` | integer | Duration in milliseconds for which a lock is held before auto-release. Defaults to `30000`. |
| `lock_index` | string | Key namespace for lock entries. Defaults to `cron_locks`. |
| `store_method` | string | Backing key-value store: `in_memory` (default; volatile, lost on shutdown) or `file_based` (persisted under `file_path`). |
| `file_path` | string | Directory for `file_based` storage. Defaults to `kv_store_data.db`. |
| `save_interval_ms` | integer | Flush cadence (ms) for `file_based` storage. Range `100`–`3600000`, defaults to `5000`. |

> `store_method` / `file_path` / `save_interval_ms` configure the shared backing store and take effect only for the first `kv` adapter built in the process; `lock_ttl_ms` and `lock_index` apply per adapter instance.

### redis

Uses Redis for distributed locking to prevent duplicate job execution across multiple engine instances. The lock TTL and key prefix are fixed by the adapter; only the connection URL is configurable.

```yaml
name: redis
config:
  redis_url: ${REDIS_URL:redis://localhost:6379}
```

| Field | Type | Description |
|---|---|---|
| `redis_url` | string | Redis connection URL. Defaults to `redis://localhost:6379`. |

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
