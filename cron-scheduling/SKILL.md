---
name: cron-scheduling
description: >-
  Schedules recurring tasks on the iii engine using registerFunction and
  registerTrigger type:'cron'. Use when building periodic jobs, cleanup tasks,
  polling loops, or any time-based automation.
---

# Cron Scheduling

Comparable to: node-cron, APScheduler, crontab

## Key Concepts

- A **Function** contains the recurring task logic
- A **Cron Trigger** fires the function on a schedule using a 7-field expression
- 7-field format: `second minute hour day month weekday year`
- The `CronModule` with `KvCronAdapter` must be enabled in iii-config.yaml
- Cron-triggered functions receive no meaningful input (use state for context)

## iii Primitives Used

| Primitive | Purpose |
|-----------|---------|
| `init(url)` | Connect worker to engine |
| `registerFunction({ id }, handler)` | Define the scheduled task |
| `registerTrigger({ type: 'cron', function_id, config })` | Bind schedule to function |
| `config: { expression }` | 7-field cron expression |

## Common Patterns

- `init('ws://localhost:49134')` -- connect to engine
- `registerFunction({ id: 'cleanup::expired-sessions' }, async () => { ... })` -- task logic
- `registerTrigger({ type: 'cron', function_id: 'cleanup::expired-sessions', config: { expression: '0 0 * * * * *' } })` -- every hour
- Common expressions: `0 */5 * * * * *` (every 5 min), `0 0 0 * * * *` (daily midnight), `0 0 9 * * 1 *` (Mondays 9 AM)

## Reference

See [references/REFERENCE.md](references/REFERENCE.md) for the full API reference with TypeScript, Python, Rust examples, expression table, and module config.

## Pattern Boundaries

- If the task involves one-time delayed execution, use `queue-processing` with a delay instead.
- If the task reacts to data changes rather than time, use `state-reactions`.
- If the scheduled task needs to expose results via HTTP, combine with `http-endpoints`.
- Stay with `cron-scheduling` when the goal is time-based recurring execution.
