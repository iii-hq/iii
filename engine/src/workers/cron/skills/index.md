---
type: index
title: iii-cron
---

# iii-cron

Schedule any registered function to run on a cron expression. The worker
exposes **no callable functions** — its entire surface is one trigger
type, `cron`, that you attach to a function via
`iii.registerTrigger({ type: 'cron', function_id, config })`. On every
firing the engine builds an event payload, optionally evaluates a
condition function, acquires a distributed lock through the configured
adapter, and invokes the target function. The lock guarantees once-only
execution across a fleet when the `redis` adapter is in use; the
default `kv` adapter only locks process-local, so multi-instance fleets
will fire the same job on every instance.

The schedule grammar is the seven-field `cron` crate dialect
(`second minute hour day month weekday year`); the year is optional and
defaults to `*`. Because the engine is parsing real cron expressions,
six- and seven-field forms both work — the leading `0` in
`0 */5 * * * *` is the seconds field, not minutes. For the full
adapter config block, the cron expression cheat-sheet, and the
multi-instance trade-offs (`kv` vs `redis`), see
[the README](../README.md).

## Trigger types

### `cron`

Fires on a schedule. The handler receives a synthetic event payload
describing which job fired and at what scheduled vs actual time. Use
when you need a pure timer — periodic cleanup, hourly reports,
heartbeat health checks, batch maintenance. The trigger has no
runtime API beyond `iii.registerTrigger` / `iii.unregisterTrigger`.

#### Config

```yaml
- type: cron
  function_id: jobs::cleanupOldData            # required; the function to invoke when the schedule fires
  config:
    expression: "0 0 2 * * * *"                # required; 6- or 7-field cron expression (sec min hour dom month dow [year])
    condition_function_id: jobs::isWeekday     # optional; gates each firing — handler is skipped when this returns false
```

`expression` is required and must parse against the `cron` crate
grammar; the worker rejects the registration synchronously when it
doesn't. `condition_function_id` is optional — when set, the engine
calls it with the same event payload the handler would receive and
runs the handler only when the condition returns `true`. A condition
that throws is treated as an error, the handler is skipped, and the
distributed lock is released (so another instance does not try to
re-fire).

#### Payload

```json
{
  "trigger":        "cron",                            // always the literal string "cron"
  "job_id":         "cleanup-old-data",                // the trigger id the engine assigned at register time
  "scheduled_time": "2026-05-20T17:00:00.000+00:00",   // when this firing was supposed to run (RFC 3339, UTC)
  "actual_time":    "2026-05-20T17:00:00.142+00:00"    // when the worker actually invoked the handler (RFC 3339, UTC)
}
```

- The pair `(scheduled_time, actual_time)` lets you measure scheduler
  drift: subtract them on the handler side to surface contention or
  GC pauses on the engine.
- `job_id` is the trigger's id — useful when one function is bound to
  multiple cron triggers (different schedules, different conditions)
  and the handler needs to dispatch on which one fired.
- The payload is the same JSON object the condition function receives,
  so condition logic can branch on `job_id` without coordinating with
  the registration site.

## How to register

The trigger type lives outside the function surface, so there is no
`iii://cron/<fn>` how-to to follow — registration is the entire
playbook. Reach for it when you need any of:

- A function to run periodically without a separate scheduler process
  or system `crontab` entry.
- Once-only firing across a multi-instance engine fleet (configure
  `adapter.name: redis` so the lock is shared; the default `kv`
  adapter is single-instance only).
- A scheduled job that should be skipped when an arbitrary condition
  is false (holiday calendar, feature-flag check, weekend pause) —
  set `condition_function_id` rather than threading the check inside
  the handler.

```typescript
import { registerWorker } from 'iii-sdk'

const iii = registerWorker('ws://localhost:49134')

const fn = iii.registerFunction(
  { id: 'jobs::cleanupOldData' },
  async (event) => {
    // event.scheduled_time, event.actual_time, event.job_id are populated.
    return {}
  },
)

iii.registerTrigger({
  type: 'cron',
  function_id: fn.id,
  config: { expression: '0 0 2 * * * *' },     // every day at 02:00:00 UTC
})
```

For the cron-expression cheat-sheet (every minute, every weekday at
9-17, every Sunday at midnight, etc.), the once-only execution
guarantee, and the per-adapter config keys (`lock_ttl_ms`,
`lock_index`, `redis_url`), see [the README](../README.md).
