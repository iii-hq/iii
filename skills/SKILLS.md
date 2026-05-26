# iii Skills

Skills for building on the [iii engine](https://iii.dev) — a backend unification and orchestration system.

## Getting Started

- [getting-started](iii-getting-started/SKILL.md) — Install iii, create a project, write your first worker

## HOWTO Skills

Direct mappings to iii documentation HOWTOs. Each teaches an iii-level primitive or engine concept.
Worker-backed capabilities live with their engine workers under `engine/src/workers/**/skills`.

- [functions-and-triggers](iii-functions-and-triggers/SKILL.md) — Register functions and triggers across TypeScript, Python, and Rust
- [custom-triggers](iii-custom-triggers/SKILL.md) — Build custom trigger types for external events
- [trigger-actions](iii-trigger-actions/SKILL.md) — Choose sync, void, or durable enqueue when deciding how work should run
- [trigger-conditions](iii-trigger-conditions/SKILL.md) — Gate trigger execution with condition functions
- [trigger-schemas](iii-trigger-schemas/SKILL.md) — Built-in trigger config and handler payload schemas
- [engine-config](iii-engine-config/SKILL.md) — Configure the iii engine via iii-config.yaml
- [channels](iii-channels/SKILL.md) — Binary streaming between workers
- [http-invoked-functions](iii-http-invoked-functions/SKILL.md) — Register external HTTP endpoints as iii functions
- [error-handling](iii-error-handling/SKILL.md) — Handle engine and SDK error codes across languages

## Architecture Pattern Skills

Compose functions, triggers, workers, and worker-backed capabilities into common backend architectures. Each includes direct code examples in its `SKILL.md`.

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
