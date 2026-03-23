---
name: cron-scheduling
description: >-
  Registers cron triggers with 7-field expressions to run functions on
  recurring schedules. Use when scheduling periodic jobs, timed automation,
  crontab replacements, cleanup routines, report generation, health checks,
  batch processing, or any task that should run every N seconds, minutes, hours,
  or on a weekly/monthly calendar.
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

| Primitive                                 | Purpose                                  |
| ----------------------------------------- | ---------------------------------------- |
| `registerFunction`                        | Define the handler for the scheduled job |
| `registerTrigger({ type: 'cron' })`       | Bind a cron expression to a function     |
| `config: { expression: '0 0 9 * * * *' }` | Cron schedule in 7-field format          |

## Reference Implementation

See [../references/cron-scheduling.js](../references/cron-scheduling.js) for the full working example — a recurring scheduled task that fires on a cron expression and optionally enqueues heavy work.

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

## Engine Configuration

CronModule must be enabled in iii-config.yaml. See [../references/iii-config.yaml](../references/iii-config.yaml) for the full annotated config reference.

## Pattern Boundaries

- If the task is about one-off async work rather than recurring schedules, prefer `queue-processing`.
- If the trigger should fire on state changes rather than time, prefer `state-reactions`.
- Stay with `cron-scheduling` when the primary need is time-based periodic execution.

## When to Use

- Use this skill when the task is primarily about `cron-scheduling` in the iii engine.
- Triggers when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.
