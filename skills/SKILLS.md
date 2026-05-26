# iii Skills

Skills for building on the [iii engine](https://iii.dev) — a backend unification and orchestration system.

## Getting Started

- [getting-started](iii-getting-started/SKILL.md) — Install iii, create a project, write your first worker
- [examples](iii-examples/SKILL.md) — Copyable examples for functions, triggers, enqueue, reactions, channels, and worker discovery

## HOWTO Skills

Direct mappings to iii documentation HOWTOs. Each teaches one iii primitive or worker-backed capability.
Use `iii-examples` when an agent needs a concrete code shape before choosing a deeper skill.

- [functions-and-triggers](iii-functions-and-triggers/SKILL.md) — Register functions and triggers across TypeScript, Python, and Rust
- [http-endpoints](iii-http-endpoints/SKILL.md) — Expose functions as REST API endpoints
- [cron-scheduling](iii-cron-scheduling/SKILL.md) — Schedule recurring tasks with cron expressions
- [queue-processing](iii-queue-processing/SKILL.md) — Enqueue slow or reliable background work with retries, concurrency, and ordering
- [state-management](iii-state-management/SKILL.md) — Distributed key-value state across functions
- [state-reactions](iii-state-reactions/SKILL.md) — React automatically when state changes: when X changes, do Y
- [realtime-streams](iii-realtime-streams/SKILL.md) — Push live updates to WebSocket clients
- [pubsub](iii-pubsub/SKILL.md) — Broadcast non-durable topic events to subscribers
- [custom-triggers](iii-custom-triggers/SKILL.md) — Build custom trigger types for external events
- [trigger-actions](iii-trigger-actions/SKILL.md) — Choose sync, void, or durable enqueue when deciding how work should run
- [trigger-conditions](iii-trigger-conditions/SKILL.md) — Gate trigger execution with condition functions
- [trigger-schemas](iii-trigger-schemas/SKILL.md) — Built-in trigger config and handler payload schemas
- [dead-letter-queues](iii-dead-letter-queues/SKILL.md) — Inspect and redrive failed queue jobs
- [engine-config](iii-engine-config/SKILL.md) — Configure the iii engine via iii-config.yaml
- [observability](iii-observability/SKILL.md) — OpenTelemetry tracing, metrics, and logging
- [channels](iii-channels/SKILL.md) — Binary streaming between workers
- [http-middleware](iii-http-middleware/SKILL.md) — Engine-level middleware for HTTP triggers (auth, logging, rate limiting)
- [http-invoked-functions](iii-http-invoked-functions/SKILL.md) — Register external HTTP endpoints as iii functions
- [error-handling](iii-error-handling/SKILL.md) — Handle engine and SDK error codes across languages
- [worker-lifecycle](iii-worker-lifecycle/SKILL.md) — Manage worker add/remove/start/stop/logs/exec commands
- [worker-lockfile](iii-worker-lockfile/SKILL.md) — Reproduce registry-managed worker installs with iii.lock
- [worker-rbac](iii-worker-rbac/SKILL.md) — Configure worker RBAC, filtered discovery, and FORBIDDEN behavior

## Worker Registry Skill

One dynamic skill covers published workers from workers.iii.dev and the iii-hq/workers source repo. Worker-specific skills live with the worker and should be loaded from the registry through `iii-directory`.

- [worker-catalog](iii-worker-catalog/SKILL.md) — Discover registry workers, install the right capability, and load worker-provided skills

## Architecture Pattern Skills

Compose functions, triggers, workers, and worker-backed capabilities into common backend architectures. Each includes a full working `reference.js`.

- [agentic-backend](iii-agentic-backend/SKILL.md) — Multi-agent pipelines with queue handoffs and shared state
- [reactive-backend](iii-reactive-backend/SKILL.md) — Change-driven backends with state triggers, stream updates, and no polling
- [workflow-orchestration](iii-workflow-orchestration/SKILL.md) — Durable multi-step pipelines with retries and DLQ
- [effect-system](iii-effect-system/SKILL.md) — Composable, traceable function pipelines
- [event-driven-cqrs](iii-event-driven-cqrs/SKILL.md) — CQRS with event sourcing and independent projections
- [low-code-automation](iii-low-code-automation/SKILL.md) — Trigger-transform-action automation chains

## SDK Reference Skills

Minimal skills pointing to official SDK documentation.

- [node-sdk](iii-node-sdk/SKILL.md) — Node.js/TypeScript SDK
- [browser-sdk](iii-browser-sdk/SKILL.md) — Browser SDK (WebSocket from web apps)
- [python-sdk](iii-python-sdk/SKILL.md) — Python SDK
- [rust-sdk](iii-rust-sdk/SKILL.md) — Rust SDK

## Shared References

- [references/iii-config.yaml](references/iii-config.yaml) — Full annotated engine configuration reference (auto-synced from docs)
