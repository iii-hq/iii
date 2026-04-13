# iii architecture — deep dive (for thought leadership)

## One-sentence thesis (product)

**iii** is a **backend unification engine**: one Rust process plus **three primitives** — **Function**, **Trigger**, **Worker** — that replace the usual sprawl of separate API servers, queues, crons, streams, state, and observability glue with a **single integration point** and a **uniform execution model** across languages (TypeScript, Python, Rust) over **JSON on WebSocket**.

## The three primitives

| Primitive | Role |
|-----------|------|
| **Function** | Unit of work: async handler identified by a stable ID (e.g. `orders::validate`). Can live in any worker process, or be modeled as an HTTP-invoked external endpoint (with reduced engine affordances). |
| **Trigger** | Binds an *event source* to a function: HTTP route, cron (`expression`, 7-field), queue subscription, state, stream, durable subscriber, etc. |
| **Worker** | (1) **Your** runtime: a long-lived (or ephemeral) process that **registers** functions and triggers with the engine over WebSocket. (2) **Engine composition:** built-in capabilities (HTTP, queue, stream, cron, state, …) are also implemented as **`Worker`** instances in the engine — same abstraction, not a separate “product tier.” See [`06-unified-worker-model-and-paradigm.md`](./06-unified-worker-model-and-paradigm.md). |

**Cross-cutting behavior:** functions invoke other functions via `trigger()`; serialization, routing, and delivery are engine concerns, not ad hoc RPC wiring in application code.

## System layers (engine) — and why “module” = worker

Docs such as [`docs/advanced/architecture.mdx`](../docs/advanced/architecture.mdx) describe **engine core**, **core modules**, and **SDK workers** for clarity. In the **Rust engine**, that distinction collapses to one idea: **everything that plugs into the engine implements the same `Worker` trait** (`engine/src/workers/traits.rs`). Built-ins are registered by name, for example:

- `iii-http` → `HttpWorker` (`rest_api/api_core.rs`)
- `iii-queue` → `QueueWorker` (`queue/queue.rs`)
- `iii-stream` → `StreamWorker` (`stream/stream.rs`)
- `iii-cron` → `CronWorker`, `iii-state` → `StateWorker`, `iii-pubsub` → `PubSubWorker`, `iii-observability` → `ObservabilityWorker`, `iii-worker-manager` → `WorkerManager`, …

So **queues are not a separate “core offering” in the architectural sense** — they are **one worker** among many. Same for streams, HTTP front door, cron, etc. New surface area (exec/shell, bridges, …) follows the same pattern (`register_worker!(...)` in `engine/src/workers/`).

**SDK processes** (Node/Python/Rust) are **workers** in product language: they register **your** functions. **Infrastructure workers** register **system** behavior. One compositional model; capabilities are **patterns** (how you combine functions, triggers, and workers), not a checklist of unrelated products.

**Redis (or similar)** often backs queue/stream/cron adapters in multi-instance setups — horizontal scaling is “many engines + shared adapters,” not a separate bespoke story per concern.

**Narrative asset:** paradigm-level framing (“design patterns, not feature pillars”) lives in [`06-unified-worker-model-and-paradigm.md`](./06-unified-worker-model-and-paradigm.md).

## Discovery (why this is not “microservices + Consul”)

From [`docs/primitives-and-concepts/discovery.mdx`](../docs/primitives-and-concepts/discovery.mdx):

- On connect, a worker receives **the full set of functions** registered in the system.
- When any worker registers or unregisters functions, **all workers get pushed updates**.
- The engine **is** the registry; topology changes are **live**, not poll/TTL based.

**Implication for narrative:** iii collapses “where does this capability live?” into a **single live catalog** of callable units — important for humans *and* for agents that need to reason about what the system can do without reading every repo’s README.

## Wire protocol and polyglot story

- SDKs (Node, Python, Rust) share a **JSON-over-WebSocket** protocol to the same engine.
- **Interoperable execution:** a function in one language `trigger()`s a function in another as if they shared one runtime; the engine handles marshalling and routing.

## Execution patterns (documentation framing)

The architecture doc distinguishes:

- **Synchronous** — HTTP (or similar) → engine → worker → response.
- **Asynchronous** — publish → queue → subscribers / durable processing.
- **Scheduled** — cron tick → lock → single execution across replicas.

These are **one model** (function + trigger) with different trigger types, rather than three frameworks stitched together.

## Framework layer: Motia

[`frameworks/motia/docs/.../iii-engine.mdx`](../frameworks/motia/docs/content/docs/concepts/iii-engine.mdx) positions Motia as the **application framework** (Steps, etc.) while the **iii engine** supplies modules (HTTP, queue, state, stream, cron, pubsub, OTel, exec). **config.yaml** is the single infrastructure surface; adapters swap (file → Redis) without rewriting app logic.

**Thought leadership angle:** iii can be discussed at the **infrastructure primitive** level; Motia is optional sugar for teams that want a higher-level step model on top.

## Agent skills (distribution of “how to build”)

The repo ships **SkillKit** skills under `skills/` (e.g. agentic backends, effect pipelines, HTTP-invoked functions). That matters for the **agent community**: iii is not only a runtime but a **documented, installable pattern library** for coding agents.

## Boundaries to respect in public writing

- **Engine** is ELv2; **SDKs and most of the repo** are Apache-2.0 — cite accurately if licensing comes up.
- Public API / protocol details should align with published docs; this note is for narrative, not normative spec.

## Glossary (for the article)

- **Function ID** — Namespaced string, often `domain::action`.
- **Trigger** — Declarative binding from event source to function (not “cron” key in config — **`expression`** in product docs).
- **HTTP api_path** — Leading slash (e.g. `/orders`), engine convention.
