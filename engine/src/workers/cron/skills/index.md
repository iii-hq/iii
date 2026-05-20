---
type: index
title: iii-cron
---

# iii-cron

Schedule any registered function to run on a cron expression. The worker exposes **no callable functions** — its entire surface is one trigger type, `cron`, that you attach to a function via `iii.registerTrigger({ type: 'cron', function_id, config })`. On every firing the engine builds an event payload, optionally evaluates a condition function, acquires a distributed lock through the configured adapter, and invokes the target function. The lock guarantees once-only execution across a fleet when the `redis` adapter is in use; the default `kv` adapter only locks process-local, so multi-instance fleets will fire the same job on every instance.

The schedule grammar is the seven-field `cron` crate dialect (`second minute hour day month weekday year`); the year is optional and defaults to `*`. Because the engine is parsing real cron expressions, six- and seven-field forms both work — the leading `0` in `0 */5 * * * *` is the seconds field, not minutes. For the full adapter config block, the cron-expression cheat-sheet, and the multi-instance trade-offs (`kv` vs `redis`), see [the README](../README.md).

- **Reactive triggers** (`cron`) — fires the bound function on a cron schedule. The worker has no other surface; everything else (registration metadata, condition gating, etc.) flows through the standard `iii.registerTrigger` path.

## How-tos

### `cron` triggers

- [Schedule a function on a cron expression](iii://iii-cron/cron/reactive-triggers) — register a handler and a `cron` trigger to fire on a 6- or 7-field cron schedule, with optional `condition_function_id` gating and per-firing `(scheduled_time, actual_time)` reporting in the event payload.
