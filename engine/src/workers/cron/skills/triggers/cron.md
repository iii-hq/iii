---
type: how-to
trigger_type: cron
title: Run a function on a cron schedule
---

# When to use

Register a `cron` trigger when you want a function to fire on a
recurring schedule, defined by a 6- or 7-field cron expression, with
once-only-execution semantics across a multi-instance engine fleet
(when paired with the `redis` adapter). The handler receives a payload
that names which scheduled run fired and at what scheduled vs. actual
time, so drift, reentrancy, and audit logging are all observable from
inside the function.

Reach for it when:

- You need a function to run periodically without standing up a
  separate scheduler process or a system `crontab` entry.
- You need once-only firing across a fleet — e.g. nightly cleanup,
  hourly reports, batch maintenance — and you've configured
  `adapter.name: redis` so the lock is shared across engine instances.
- You want a scheduled job that is conditionally skipped (holiday
  calendar, feature flag, weekend pause) without threading the check
  inside the handler — set `condition_function_id` and the engine
  evaluates it before each firing.

Use a `state` (`iii://state/triggers/state`) trigger instead when the
work should fire on data changes rather than on the clock. Use a
`stream` (`iii://stream/triggers/stream`) trigger instead when the
work should fire on stream item changes or WebSocket lifecycle events.

# Inputs

The trigger config passed to `iii.registerTrigger({ type: 'cron',
function_id, config })`:

```json
{
  "expression":            "0 0 2 * * * *",            // required; 6- or 7-field cron expression: sec min hour dom month dow [year]
  "condition_function_id": "jobs::isWeekday"           // optional; gate. The condition function is invoked with the same payload the handler would receive; the handler runs only when it returns truthy.
}
```

`expression` is required and must parse against the `cron` crate
grammar. The worker rejects the registration synchronously when the
expression is missing, empty, or unparseable; the error surfaces as
the `Err` from `iii.registerTrigger`.

The supported grammar is **6- or 7-field**, ordered:

```
second  minute  hour  day-of-month  month  day-of-week  [year]
```

The year field is optional. Six-field expressions imply `year = *`.
Common patterns:

| Expression          | Meaning                                                  |
|---------------------|----------------------------------------------------------|
| `0 * * * * *`       | Every minute (at second 0).                              |
| `0 0 * * * *`       | Every hour (at minute 0, second 0).                      |
| `0 0 2 * * *`       | Every day at 02:00:00.                                   |
| `0 0 0 * * 0 *`     | Every Sunday at 00:00:00.                                |
| `0 */5 * * * *`     | Every 5 minutes (note: the leading `0` is **seconds**, not minutes — this fires at second 0, every 5 minutes). |
| `0 0 9-17 * * 1-5 *`| Every hour from 09:00 to 17:00, Monday–Friday.           |

Reading a cron expression as if the leading position were minutes (the
five-field convention many tools use) will produce schedules that fire
60× more often than intended. Always count fields and remember that
position 0 is seconds.

`condition_function_id` is optional. When set, the engine calls the
condition function with the same event payload the handler would
receive and runs the handler only when the condition returns `true`.
A condition that errors is logged and the handler is skipped (the
distributed lock is released either way, so another instance does not
attempt to re-fire the same scheduled run).

# Outputs

The handler function receives the cron event payload:

```json
{
  "trigger":        "cron",                            // always the literal string "cron"
  "job_id":         "jobs::cleanupOldData",            // the trigger id (same id you passed via `iii.registerTrigger({ id })` or the auto-derived id when you didn't pass one)
  "scheduled_time": "2026-05-20T17:00:00.000+00:00",   // when this firing was supposed to run, RFC 3339 UTC
  "actual_time":    "2026-05-20T17:00:00.142+00:00"    // when the worker actually invoked the handler, RFC 3339 UTC
}
```

- `trigger` is always the literal `"cron"`. Used to discriminate when
  one function is bound to multiple trigger types.
- `job_id` is the trigger's id. Useful when a single function is bound
  to multiple cron triggers (different schedules, different conditions)
  and the handler needs to dispatch on which one fired — for example,
  a reporting function bound to both an hourly summary trigger and a
  daily rollup trigger.
- `scheduled_time` is the moment the cron schedule predicted this run
  for. `actual_time` is the moment the worker actually invoked the
  handler. Subtract `actual_time - scheduled_time` to surface
  scheduler drift caused by GC pauses, lock contention on the
  distributed adapter, or system load. Drift consistently above a few
  hundred milliseconds is a sign the engine is overloaded or the
  `kv` adapter is being hit cross-instance.
- The same payload is passed to the condition function (when one is
  configured), so condition logic can branch on `job_id` or
  `scheduled_time` without coordinating with the registration site.

The handler's return value is ignored by the cron worker — `cron`
triggers are fire-and-forget. Errors from the handler are logged but
do not affect the schedule; the next firing proceeds normally.

# Worked example

Register a daily 02:00 UTC cleanup function:

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

Gate firing on a runtime condition (skip on weekends):

```typescript
const isWeekday = iii.registerFunction(
  { id: 'jobs::isWeekday' },
  async (event) => {
    const day = new Date(event.scheduled_time).getUTCDay()  // 0 = Sun, 6 = Sat
    return day >= 1 && day <= 5
  },
)

iii.registerTrigger({
  type: 'cron',
  function_id: 'jobs::generateDailyReport',
  config: {
    expression: '0 0 9 * * * *',               // 09:00 UTC daily
    condition_function_id: isWeekday.id,
  },
})
```

Multiple triggers bound to one handler, dispatched on `job_id`:

```typescript
const reportFn = iii.registerFunction(
  { id: 'jobs::generateReport' },
  async (event) => {
    if (event.job_id === 'jobs::generateReport.hourly') {
      // hourly summary
    } else if (event.job_id === 'jobs::generateReport.daily') {
      // daily rollup
    }
    return {}
  },
)

iii.registerTrigger({
  id: 'jobs::generateReport.hourly',
  type: 'cron',
  function_id: reportFn.id,
  config: { expression: '0 0 * * * *' },       // every hour
})

iii.registerTrigger({
  id: 'jobs::generateReport.daily',
  type: 'cron',
  function_id: reportFn.id,
  config: { expression: '0 0 0 * * * *' },     // every day at 00:00:00
})
```

# Related

- `iii-cron` adapter config (see [the README](../../README.md)) — the once-only-execution guarantee depends on `adapter.name: redis` for multi-instance deployments. The default `kv` adapter only locks process-local, so each engine instance fires the same scheduled run.
- `state` reactive trigger (`iii://state/triggers/state`) — fire on data changes instead of on the clock.
- `stream` reactive trigger (`iii://stream/triggers/stream`) — fire on stream item changes; pair with `cron` for "every hour, refresh the projection of the last hour's stream events."
