# Everything is a worker — why the model wins

This captures a **central thesis** for thought leadership: iii is not a bag of **separate product pillars** (queue product + stream product + API product). **Composition is unified**: engine capabilities and user runtimes share one conceptual model — **Worker** — so **functionality becomes design patterns**, not a shopping list of disjoint systems.

## The claim

- **Queues** are **a worker** (`iii-queue` / `QueueWorker`), not “the core of iii.”
- **Streams** are **a worker** (`iii-stream` / `StreamWorker`).
- **HTTP**, **cron**, **state**, **pubsub**, **observability**, **worker manager** (WebSocket channel for SDK workers) — each is a **named worker** plugged into the same engine lifecycle (`create` → `initialize` → `register_functions` → background tasks → `destroy`).
- **Your application** connects as **workers** too (SDK processes registering functions).

So **“Worker” is not only “the thing that runs my business code.”** It is the **unit of composition for the whole engine**. Capabilities differ by **what each worker does**, not by **a different meta-architecture per feature**.

## Grounding in code (for factual discipline)

- Unified trait: `Worker` in `engine/src/workers/traits.rs`.
- Registration pattern: `register_worker!("iii-queue", QueueWorker, ...)`, `register_worker!("iii-stream", StreamWorker, ...)`, `register_worker!("iii-http", HttpWorker, ...)`, etc. — see `engine/src/workers/*/`.
- Engine startup walks **worker configs** and instantiates each module the same way (`engine/src/workers/config.rs` — workers list includes both user-facing naming and built-ins).

## What this is *not* saying

- It does not mean queues are unimportant — it means they are **not a separate ontology**. They are **one composable worker** with adapters (builtin, Redis, RabbitMQ, …), same as other workers.
- **Sandboxing** (or any future isolation story) fits the same story: **implemented as a worker / bridge / exec path**, not as a bolt-on “sandbox SKU.” (No `sandbox` symbol in engine today — use as **pattern** language for the article, not a shipped product name, unless product confirms.)

## Why this wins (paradigm-shift framing)

Historical shifts in software often look like: **one unifying abstraction** replaces many ad hoc categories (e.g. “everything is a file,” “everything is an object,” UI as a tree of components). Here: **everything that participates in the runtime is a worker** — infrastructure and application code **meet in one model**.

**Implications for the post:**

| Old framing | iii framing |
|-------------|-------------|
| “We bought a queue and an API gateway and a scheduler…” | “We compose **workers**; queue/stream/http are **the same kind of thing**.” |
| Feature roadmap as separate pillars | **Design patterns** on top of Function + Trigger + Worker |
| Integration tax between products | One engine lifecycle, one registry, one discovery story |

## Tie-in to harness debate (`05`)

The industry argues **thick vs thin LLM harness**. iii’s parallel claim is orthogonal: **thick vs thin backend** is the wrong axis if **backend pieces don’t share a primitive**. Unifying on **Worker** (plus Function + Trigger) is how **execution infrastructure** stays **coherent** while harnesses above churn.

## Drafting hooks (one-liners)

- *Queues aren’t the product — composable workers are.*
- *In iii, “infrastructure” isn’t a separate catalog; it’s workers all the way down.*
- *Paradigm shifts don’t add features — they collapse categories.*

## Related files

- Technical detail: [`01-iii-architecture-deep-dive.md`](./01-iii-architecture-deep-dive.md)
- Harness spectrum: [`05-harness-spectrum-and-iii-thesis.md`](./05-harness-spectrum-and-iii-thesis.md)
