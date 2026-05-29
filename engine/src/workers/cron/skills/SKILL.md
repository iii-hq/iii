---
name: iii-cron
description: >-
  Schedule any registered function on a 6- or 7-field cron expression, with
  once-only execution across a fleet when backed by the redis adapter. Its whole
  surface is the `cron` trigger type.
---

# iii-cron

The `iii-cron` worker schedules a registered function to run on a recurring cron expression. It exposes no callable functions — its entire surface is one trigger type, `cron`, bound via `iii.registerTrigger({ type: 'cron', function_id, config })`. On every firing the engine builds an event payload, optionally evaluates a condition function, acquires a distributed lock through the configured adapter, and invokes the target function. Each firing reports `scheduled_time` vs. `actual_time` so drift and reentrancy are observable from inside the handler.

The schedule grammar is the seven-field cron dialect — `second minute hour day-of-month month day-of-week [year]` — where the year is optional and defaults to `*`. Both six- and seven-field forms work; the leading field is always seconds, so `0 */5 * * * *` fires every 5 minutes at second 0, not every 5 seconds.

Two adapters govern once-only execution: `kv` (default) takes a process-local lock and is single-instance only — on a multi-instance fleet every engine fires the same job (tunables `lock_ttl_ms`, `lock_index`); `redis` takes a distributed lock and is required for once-only firing across a fleet (tunable `redis_url`).

## When to Use

- A function should run periodically without standing up a separate scheduler process or a system crontab entry.
- You need once-only firing across a fleet — nightly cleanup, hourly reports, batch maintenance — paired with the `redis` adapter.
- A scheduled job should be conditionally skipped (holiday calendar, feature flag, weekend pause) via `condition_function_id` without threading the check through the handler.

## Boundaries

- No callable functions — never invoked through a `cron::*` id; everything flows through `iii.registerTrigger`.
- The default `kv` adapter only locks process-local; never rely on it for once-only jobs in a multi-instance deployment — use `redis`.
- Reading the leading field as minutes (the five-field crontab convention) schedules jobs 60x too often; always count fields and remember position 0 is seconds.
- For data-change or stream-change reactions use `iii-state` / `iii-stream`; `iii-cron` fires on the clock only.

## Reactive triggers

Bind a `cron` trigger when a handler should run on a recurring schedule. The handler runs server-side on a tokio task spawned by the engine; with the `redis` adapter it fires once across the fleet per scheduled run.

Reach for it when:

- You need recurring execution (cleanup, reports, maintenance) without an external scheduler.
- You want condition-gated firing: set `condition_function_id` and the engine evaluates it before each run, skipping the handler (and releasing the lock) on a falsy or erroring result.

### How to bind

1. Register a handler: `iii.registerFunction('jobs::cleanup-old-data', handler)`.
2. Register the trigger:

```typescript
iii.registerTrigger({
  type: 'cron',
  function_id: 'jobs::cleanup-old-data',
  config: {
    expression: '0 0 2 * * * *',  // required. sec min hour dom month dow [year]; daily at 02:00:00.
    // condition_function_id is also supported.
  },
})
```

`expression` is required and must parse, or registration fails synchronously. Bind one `function_id` to several triggers with distinct ids to drive multiple schedules into one handler — the trigger id arrives as `job_id` in the event. The handler's return value is ignored.

For the firing event payload (`trigger`, `job_id`, `scheduled_time`, `actual_time`), call `iii get function info` on the trigger type or handler function id.
