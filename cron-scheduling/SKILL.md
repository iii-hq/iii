---
name: cron-scheduling
description: >-
  Schedules recurring tasks with cron expressions. Use when running periodic
  cleanup, reports, health checks, or batch jobs.
---

# Cron Scheduling

Comparable to: node-cron, APScheduler, crontab

## Key Concepts

Use the concepts below when they fit the task. Not every scheduled job needs all of them.

- Cron expressions use a **7-field format**: `second minute hour day month weekday year`
- **CronModule** evaluates expressions and fires triggers on schedule
- Handlers should be **fast** — enqueue heavy work to a queue instead of blocking the cron handler
- Each cron trigger binds one expression to one function
- Overlapping schedules are fine; each trigger fires independently

## Architecture

    CronModule timer tick
      → registerTrigger type:'cron' expression match
        → registerFunction handler
          → (optional) TriggerAction.Enqueue for heavy work

## iii Primitives Used

| Primitive                                        | Purpose                                      |
| ------------------------------------------------ | -------------------------------------------- |
| `registerFunction`                               | Define the handler for the scheduled job     |
| `registerTrigger({ type: 'cron' })`              | Bind a cron expression to a function         |
| `config: { expression: '0 0 9 * * * *' }`        | Cron schedule in 7-field format              |

## Reference Implementation

See [references/reference.js](references/reference.js) for the full working example — a recurring scheduled task that fires on a cron expression and optionally enqueues heavy work.

## Common Patterns

Code using this pattern commonly includes, when relevant:

- `registerWorker(url, { workerName })` — worker initialization
- `registerFunction(id, handler)` — define the scheduled handler
- `registerTrigger({ type: 'cron', config: { expression } })` — bind the schedule
- `trigger({ function_id, payload, action: TriggerAction.Enqueue({ queue }) })` — offload heavy work
- `const logger = new Logger()` — structured logging per job

## Adapting This Pattern

Use the adaptations below when they apply to the task.

- Adjust the 7-field expression to match your schedule (e.g. `0 0 */6 * * * *` for every 6 hours)
- Keep the cron handler lightweight — use it to validate and enqueue, not to do the heavy lifting
- For jobs that need state (e.g. last-run timestamp), combine with `state-management`
- Multiple cron triggers can feed the same queue for fan-in processing

## Pattern Boundaries

- If the task is about one-off async work rather than recurring schedules, prefer `queue-processing`.
- If the trigger should fire on state changes rather than time, prefer `state-reactions`.
- Stay with `cron-scheduling` when the primary need is time-based periodic execution.
