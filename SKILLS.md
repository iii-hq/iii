# iii Skills

Skills for building on the [iii engine](https://iii.dev) — a backend unification and orchestration system.

## HOWTO Skills

Direct mappings to iii documentation HOWTOs. Each teaches one primitive or capability.

- [functions-and-triggers](functions-and-triggers/SKILL.md) — Register functions and triggers across TypeScript, Python, and Rust
- [http-endpoints](http-endpoints/SKILL.md) — Expose functions as REST API endpoints
- [cron-scheduling](cron-scheduling/SKILL.md) — Schedule recurring tasks with cron expressions
- [queue-processing](queue-processing/SKILL.md) — Async job processing with retries, concurrency, and ordering
- [state-management](state-management/SKILL.md) — Distributed key-value state across functions
- [state-reactions](state-reactions/SKILL.md) — Auto-trigger functions on state changes
- [realtime-streams](realtime-streams/SKILL.md) — Push live updates to WebSocket clients
- [custom-triggers](custom-triggers/SKILL.md) — Build custom trigger types for external events
- [trigger-actions](trigger-actions/SKILL.md) — Synchronous, fire-and-forget, and enqueue invocation modes
- [trigger-conditions](trigger-conditions/SKILL.md) — Gate trigger execution with condition functions
- [dead-letter-queues](dead-letter-queues/SKILL.md) — Inspect and redrive failed queue jobs
- [engine-config](engine-config/SKILL.md) — Configure the iii engine via iii-config.yaml
- [observability](observability/SKILL.md) — OpenTelemetry tracing, metrics, and logging

## Architecture Pattern Skills

Compose multiple iii primitives into common backend architectures. Each includes a full working `reference.js`.

- [agentic-backend](agentic-backend/SKILL.md) — Multi-agent pipelines with queue handoffs and shared state
- [reactive-backend](reactive-backend/SKILL.md) — Real-time backends with state triggers and stream updates
- [workflow-orchestration](workflow-orchestration/SKILL.md) — Durable multi-step pipelines with retries and DLQ
- [http-invoked-functions](http-invoked-functions/SKILL.md) — Register external HTTP endpoints as iii functions
- [effect-system](effect-system/SKILL.md) — Composable, traceable function pipelines
- [event-driven-cqrs](event-driven-cqrs/SKILL.md) — CQRS with event sourcing and independent projections
- [low-code-automation](low-code-automation/SKILL.md) — Trigger-transform-action automation chains

## SDK Reference Skills

Minimal skills pointing to official SDK documentation.

- [node-sdk](node-sdk/SKILL.md) — Node.js/TypeScript SDK
- [python-sdk](python-sdk/SKILL.md) — Python SDK
- [rust-sdk](rust-sdk/SKILL.md) — Rust SDK

## Shared References

- [references/iii-config.yaml](references/iii-config.yaml) — Full annotated engine configuration reference (auto-synced from docs)
