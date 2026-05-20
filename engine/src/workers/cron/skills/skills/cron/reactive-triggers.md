---
type: how-to
trigger_type: cron
title: Schedule a function on a cron expression
---

# When to use

Register a `cron` trigger when a function should run on a recurring schedule defined by a 6- or 7-field cron expression. The handler runs server-side, on a tokio task spawned by the engine, with once-only-execution semantics across a multi-instance fleet (when paired with the `redis` adapter). Each firing receives a payload that names which scheduled run fired and at what scheduled vs. actual time, so drift, reentrancy, and audit logging are all observable from inside the function.

Reach for it when:

- You need a function to run periodically without standing up a separate scheduler process or a system `crontab` entry.
- You need once-only firing across a fleet — nightly cleanup, hourly reports, batch maintenance — and you've configured `adapter.name: redis` so the lock is shared across engine instances.
- You want a scheduled job that is conditionally skipped (holiday calendar, feature flag, weekend pause) without threading the check inside the handler — set `condition_function_id` and the engine evaluates it before each firing.

Use a `state` reactive trigger instead when the work should fire on data changes rather than on the clock. Use a `stream` reactive trigger instead when the work should fire on stream item changes or WebSocket lifecycle events.

Prerequisite: the `iii-cron` worker must be enabled in `config.yaml` (it is on by default). Handlers and triggers are registered from a connected worker via `iii.registerFunction` and `iii.registerTrigger` — the worker has no `cron::*` engine functions.

# Inputs

Registration is a two-step pattern: define the handler function, then bind it to the `cron` trigger type.

## Handler function

Register any function id. The handler receives the event payload documented in Outputs (same shape the engine passes to `condition_function_id` when configured).

```json
// iii.registerFunction — handler id only; no engine payload.
{ "id": "jobs::cleanup-old-data" }
```

## Trigger registration

```json
{
  "type":        "cron",                              // required. Must be exactly "cron".
  "function_id": "jobs::cleanup-old-data",            // required. Handler invoked on every firing.
  "config": {
    "expression":            "0 0 2 * * * *",         // required. 6- or 7-field cron expression: sec min hour dom month dow [year].
    "condition_function_id": "jobs::is-weekday"       // optional. Engine invokes this with the event; handler runs only on truthy.
  }
}
```

`type`, `function_id`, and `config.expression` are required. `expression` cannot be missing or empty — the worker rejects the registration synchronously when it can't parse it; the error surfaces as the `Err` from `iii.registerTrigger`.

The supported grammar is **6- or 7-field**, ordered:

```
second  minute  hour  day-of-month  month  day-of-week  [year]
```

The year field is optional. Six-field expressions imply `year = *`. Common patterns:

| Expression          | Meaning                                                                                          |
|---------------------|--------------------------------------------------------------------------------------------------|
| `0 * * * * *`       | Every minute (at second 0).                                                                      |
| `0 0 * * * *`       | Every hour (at minute 0, second 0).                                                              |
| `0 0 2 * * *`       | Every day at 02:00:00.                                                                           |
| `0 0 0 * * 0 *`     | Every Sunday at 00:00:00.                                                                       |
| `0 */5 * * * *`     | Every 5 minutes — note the leading `0` is **seconds**, not minutes; this fires at second 0, every 5 minutes. |
| `0 0 9-17 * * 1-5 *`| Every hour from 09:00 to 17:00, Monday–Friday.                                                   |

Reading a cron expression as if the leading position were minutes (the five-field convention many tools use) will produce schedules that fire 60× more often than intended. Always count fields and remember that position 0 is seconds.

`condition_function_id` is optional. When set, the engine calls the condition function with the same event payload the handler would receive and runs the handler only when the condition returns `true`. A condition that errors is logged and the handler is skipped (the distributed lock is released either way, so another instance does not attempt to re-fire the same scheduled run).

# Outputs

When the schedule fires, the engine invokes `function_id` with this event object:

```json
{
  "trigger":        "cron",                            // Always the literal string "cron".
  "job_id":         "jobs::cleanup-old-data",          // The trigger id (same id you passed via `iii.registerTrigger({ id })` or the auto-derived id when you didn't pass one).
  "scheduled_time": "2026-05-20T17:00:00.000+00:00",   // When this firing was supposed to run, RFC 3339 UTC.
  "actual_time":    "2026-05-20T17:00:00.142+00:00"    // When the worker actually invoked the handler, RFC 3339 UTC.
}
```

- `trigger` is always the literal `"cron"`. Used to discriminate when one function is bound to multiple trigger types.
- `job_id` is the trigger's id. Useful when a single function is bound to multiple cron triggers (different schedules, different conditions) and the handler needs to dispatch on which one fired — for example, a reporting function bound to both an hourly summary trigger and a daily rollup trigger.
- `scheduled_time` is the moment the cron schedule predicted this run for. `actual_time` is the moment the worker actually invoked the handler. Subtract `actual_time - scheduled_time` to surface scheduler drift caused by GC pauses, lock contention on the distributed adapter, or system load. Drift consistently above a few hundred milliseconds is a sign the engine is overloaded or the `kv` adapter is being hit cross-instance.
- The same payload is passed to the condition function (when one is configured), so condition logic can branch on `job_id` or `scheduled_time` without coordinating with the registration site.

The handler's return value is **ignored**. Errors from the handler are logged but do not affect the schedule; the next firing proceeds normally.

# Worked example

Schedule a daily 02:00 UTC cleanup function.

Register the handler:

```json
// iii.registerFunction — handler id only; no engine payload.
{ "id": "jobs::cleanup-old-data" }
```

Register the trigger:

```json
{
  "type":        "cron",
  "function_id": "jobs::cleanup-old-data",
  "config":      { "expression": "0 0 2 * * * *" }
}
```

When the cron fires the handler receives:

```json
{
  "trigger":        "cron",
  "job_id":         "jobs::cleanup-old-data",
  "scheduled_time": "2026-05-21T02:00:00.000+00:00",
  "actual_time":    "2026-05-21T02:00:00.038+00:00"
}
```

Gate firing on a runtime condition (skip on weekends). Register a condition function alongside the handler:

```json
{ "id": "jobs::is-weekday" }
```

Then bind both:

```json
{
  "type":        "cron",
  "function_id": "jobs::generate-daily-report",
  "config": {
    "expression":            "0 0 9 * * * *",
    "condition_function_id": "jobs::is-weekday"
  }
}
```

`jobs::is-weekday` receives the same event payload (`trigger`, `job_id`, `scheduled_time`, `actual_time`); when it returns `false` the handler is skipped and the lock is released so the next firing proceeds normally.

Bind multiple triggers to one handler and dispatch on `job_id` inside the handler:

```json
{
  "type":        "cron",
  "function_id": "jobs::generate-report",
  "config":      { "expression": "0 0 * * * *" },
  "id":          "jobs::generate-report.hourly"
}
```

```json
{
  "type":        "cron",
  "function_id": "jobs::generate-report",
  "config":      { "expression": "0 0 0 * * * *" },
  "id":          "jobs::generate-report.daily"
}
```

The handler then sees `event.job_id` of either `"jobs::generate-report.hourly"` or `"jobs::generate-report.daily"` and can branch accordingly.

# Related

- `iii-cron` adapter config (see [the README](../../../README.md)) — the once-only-execution guarantee depends on `adapter.name: redis` for multi-instance deployments. The default `kv` adapter only locks process-local, so each engine instance fires the same scheduled run.
- `state` reactive trigger — fire on data changes instead of on the clock.
- `stream` reactive trigger — fire on stream item changes; pair with `cron` for "every hour, refresh the projection of the last hour's stream events."
