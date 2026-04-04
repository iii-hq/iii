# iii Skills

Skills for building on the [iii engine](https://iii.dev) — a backend unification and orchestration system.

## Getting Started

- [getting-started](iii-getting-started/SKILL.md) — Install iii, create a project, write your first worker

## HOWTO Skills

Direct mappings to iii documentation HOWTOs. Each teaches one primitive or capability.

- [functions-and-triggers](iii-functions-and-triggers/SKILL.md) — Register functions and triggers across TypeScript, Python, and Rust
- [http-endpoints](iii-http-endpoints/SKILL.md) — Expose functions as REST API endpoints
- [cron-scheduling](iii-cron-scheduling/SKILL.md) — Schedule recurring tasks with cron expressions
- [queue-processing](iii-queue-processing/SKILL.md) — Async job processing with retries, concurrency, and ordering
- [state-management](iii-state-management/SKILL.md) — Distributed key-value state across functions
- [state-reactions](iii-state-reactions/SKILL.md) — Auto-trigger functions on state changes
- [realtime-streams](iii-realtime-streams/SKILL.md) — Push live updates to WebSocket clients
- [custom-triggers](iii-custom-triggers/SKILL.md) — Build custom trigger types for external events
- [trigger-actions](iii-trigger-actions/SKILL.md) — Synchronous, fire-and-forget, and enqueue invocation modes
- [trigger-conditions](iii-trigger-conditions/SKILL.md) — Gate trigger execution with condition functions
- [dead-letter-queues](iii-dead-letter-queues/SKILL.md) — Inspect and redrive failed queue jobs
- [engine-config](iii-engine-config/SKILL.md) — Configure the iii engine via iii-config.yaml
- [observability](iii-observability/SKILL.md) — OpenTelemetry tracing, metrics, and logging
- [channels](iii-channels/SKILL.md) — Binary streaming between workers
- [http-middleware](iii-http-middleware/SKILL.md) — Engine-level middleware for HTTP triggers (auth, logging, rate limiting)

## Architecture Pattern Skills

Compose multiple iii primitives into common backend architectures. Each includes a full working `reference.js`.

- [agentic-backend](iii-agentic-backend/SKILL.md) — Multi-agent pipelines with queue handoffs and shared state
- [reactive-backend](iii-reactive-backend/SKILL.md) — Real-time backends with state triggers and stream updates
- [workflow-orchestration](iii-workflow-orchestration/SKILL.md) — Durable multi-step pipelines with retries and DLQ
- [http-invoked-functions](iii-http-invoked-functions/SKILL.md) — Register external HTTP endpoints as iii functions
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
