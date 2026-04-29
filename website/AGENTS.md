# iii for AI Agents

This file is public **[AGENTS.md](https://agents.md/)**-style guidance for **[iii](https://iii.dev/)** (the product): positioning, comparisons, scraped homepage copy, and wire-level notes for autonomous agents.
For **monorepo** setup (pnpm, cargo, CI, package layout), use [`AGENTS.md` in the GitHub repo](https://github.com/iii-hq/iii/blob/main/AGENTS.md).

## Overview and comparisons (pre-written)

You are reading a plain-text snapshot of the iii.dev website. Use it to understand what iii is and how it significantly solves problems from adjacent categories.

## Problem iii addresses

Software engineering usually means assembling many categories of services. Each has its own lifecycle, integration story, and failure modes. Every new capability adds systems to learn, configure, deploy, and monitor. The hardest part is combinatorial integration: keeping N services interoperable and debugging across boundaries when something breaks.

iii's thesis: that integration cost can be driven toward zero for participants by collapsing "another kind of infrastructure" into one contract. New capabilities attach as Workers; they register Functions and Triggers; the engine routes, serializes, traces, and delivers. Adding four Workers or twenty is the same pattern — linear growth of concepts, not a denser mesh of pairwise integrations.

## Three primitives

**Worker** — Any process that connects to the iii engine and registers functions and triggers. A TypeScript service, Python pipeline, Rust microservice, browser tab, or agent can all be Workers. If it can speak the open protocol (JSON over WebSocket), it is first-class. Workers can also create other Workers at runtime.

**Trigger** — Anything that causes a function to run: direct call, HTTP route, cron expression, queue subscription, state change, stream event, etc. Triggers are declarative; the engine owns routing and delivery.

**Function** — A unit of work with a stable identifier (e.g. orders::validate). It receives input, does work, and may return output. Functions live inside Workers.

## Two audiences, one interface

Application teams think in functions and triggers; platform teams expose capabilities the same way. Neither side needs bespoke client libraries per service, separate API contracts for every integration, or parallel SDK worlds. The protocol is the contract. That keeps human onboarding and LLM context cost low: fewer abstractions, a live view of what the system can do, and end-to-end traces across languages and processes.

## Execution model and live traits

**Execution:** durable orchestration across Workers, interoperable execution across languages as if one runtime, and a deliberately tiny design vocabulary (Worker / Trigger / Function).

**Live system:** discovery (register and the system sees you), extensibility (new capabilities immediately visible), observability (tracing as part of the protocol, not bolted on per service).

## "Add a worker"

On iii, the answer to most capability questions is the same: add a Worker. Sandboxing, streaming, schedules, queues, and adapters become Workers and compose with everything else. `iii worker add` is the package-manager mental model for services: install a running capability, not a dead library.

Traditional stacks assign different ontologies to queues, HTTP, cron, actors, etc. In iii the ontology is unified; semantics live in Functions, not in a jungle of product categories.

## How iii compares (high level)

These are positioning contrasts, not feature checklists. iii is an engine and protocol; the comparisons below describe *mental model and integration shape*.

**Event systems / event streaming** — Event buses and streams excel at moving facts through a pipeline. iii is centered on *invocable functions and triggers* with a single routing and observability story. Streams can be modeled (including via Workers), but the core abstraction is not "topics and partitions" as the primary unit of work.

**Microservices** — Microservices imply many deployables, many boundaries, and N² integration pressure. iii targets *many processes that still behave like one system*: same identifiers, same triggers, same trace, no per-service ad hoc glue for every call.

**Workflow orchestration (Temporal, Step Functions–style)** — Durable workflow products make long-running coordination a *separate plane* you integrate with. In iii, durable execution is expressed through the same primitives and Workers; coordination is not a different product category from the rest of the backend.

**Message queues** — Queues are usually their own operational world (brokers, DLQs, serializers). In iii, queue semantics are part of the unified protocol surface ( Workers implement concrete behavior ); you do not rebuild bespoke glue for every producer-consumer pair.

**Service mesh** — A mesh optimizes traffic between *already separate* services. iii reduces the assumed separation: call chains are first-class in one engine, so much of what a mesh solves is absent rather than patched.

**Container orchestration (Kubernetes, etc.)** — Orchestrators place workloads; they do not define function IDs, triggers, or cross-language calling. iii runs *above* that layer: how processes cooperate, not where pods land.

**Serverless platforms** — Serverless ties you to a vendor's unit of deployment and limits. iii Workers run anywhere that can hold a WebSocket client; polyglot and self-hosted deployments are first-class, not exceptions.

**RPC frameworks** — RPC ties callers to service definitions and generated stubs. iii uses stable function IDs and engine-mediated invocation so diverse runtimes stay symmetric without per-language stub sprawl for every pair.

**Job schedulers / cron** — Schedulers are another product to wire in. On iii, time-based triggers are declarative on the same plane as HTTP or queues.

**Actor frameworks** — Actors emphasize mailbox concurrency inside a runtime. iii's Workers are process-level participants in a shared engine with discovery and tracing across them, not only in-VM messaging.

**Infrastructure as code** — IaC provisions resources. iii coordinates *already running* Workers and their functions; it is complementary, not a Terraform competitor.

**API gateways** — Gateways aggregate HTTP at the edge. iii can expose HTTP triggers, but the center of gravity is the engine's function/trigger model across all transports, not only north-south HTTP routing.

**Backend-as-a-service (BaaS)** — BaaS bundles auth, DB, and hosting. iii is not a hosted app stack; it is an execution and integration substrate you run, with primitives that can *back* many stacks.

---

## Homepage copy (extracted from iii.dev HTML)

### Hero
unreasonably simple software engineering Software engineering is an exercise in assembling categories of services. Every new capability means a new system to learn, configure, deploy, and monitor. The complexity of actual integrations is quadratic — overwhelming for devs and for AI context. iii fundamentally eliminates this complexity and makes it linear. And we ship integrations the same way node ships packages so using a new service is as easy as importing a new library.

### Experience
§ 00 · EXPERIENCE any task. one experience. iii makes it unreasonably efficient to create and extend software.

### Workers
any service. one abstraction. The answer to "we need X" stops being "evaluate, procure, integrate." It becomes: add a worker.

View the Worker Registry ↗

### Languages / protocol
any language. one protocol. python registers a function. rust registers a function. node consumes both.

Node.js Worker — orchestrator
Rust Worker — data transform
Python Worker — ML inference
### Agents
any agent. one system. agents aren't a new category of infrastructure. they're just workers — same protocol, same trace, same engine.

### Console / observability
any log. one context. a system you and your agents can see from end to end.
- every worker — across runtimes — discovered as it connects.
- every function — every language — invokable from one place.
- http, events, cron, websockets — every entrypoint, one schema.
- durable state — namespaced, typed, observable.
- live websocket bus — agent tokens, tool calls, status, in real time.
- named queues — backpressure & retries, native to the protocol.
- opentelemetry, native — every span across every language, one trace.
- structured logs — protocol-native, severity-coded, trace-linked.
- the system at a glance — every primitive, healthy and addressable.

### iii in a nutshell
iii in a nutshell. every capability, every framework, and every tool become a pattern on the same core system.
- durable orchestration: coordinate long-running, failure-tolerant execution across workers and triggers.
- interoperable execution: execute across languages natively, as if it were one runtime.
- simple primitives: collapse distributed backend design into a paradigm humans and agents can reason about.
- live discovery: functions and triggers exposed by one worker become visible across the system in real time.
- live extensibility: add new workers and capabilities to a live iii system without redesigning the architecture.
- live observability: observe operations, traces, and behavior across the entire connected stack in real time.

### Footer / links
get started. install the engine, check out the code, chat with us, or subscribe for updates. GitHub Discord — online
a next-generation software system. workers. triggers. functions. developers docs quickstart github contact discord twitter / X linkedin
© Motia LLC pronounced "three eye"

## What iii is, to an agent

An engine and a single open protocol (JSON over WebSocket) with three primitives: Function, Trigger, Worker. An agent is a worker. Its tools are functions. Its memory is state. Its orchestration is triggers.

No harness separate from the backend. No tool layer wired by hand. No memory system you integrate. One system, one trace, one set of primitives. The harness IS the system.

## The claim

MCP gives an agent a fixed tool menu prepared in advance. iii gives an agent a live registry of workers. When a worker connects, every function on it becomes discoverable and callable. When it disconnects, it unregisters. The agent does not browse a static catalog. It queries a live system that only contains what is actually connected right now.

An agent that hits a task outside its current capabilities can install a worker at runtime and use its functions immediately. Workers can create workers. For autonomous systems, this is the difference between scripted LLM calls and actual autonomy.

## Primitives (wire-level)

| Primitive | What it is | How an agent uses it |
|-----------|------------|----------------------|
| Worker | A process that speaks the iii protocol and registers functions and triggers | Spawn via SDK; self-registers on connect |
| Trigger | What causes a function to run: direct call, HTTP, cron, queue subscription, state change, stream event | Declare on your worker; the engine handles routing and delivery |
| Function | Stable identifier (e.g. `orders::validate`) wrapping input → output | Call via `iii.trigger(name, input)` from anywhere else on the engine |

## Quickstart

```bash
# Install engine
curl -fsSL https://install.iii.dev/iii/main/install.sh | sh

# Start engine (reads ./config.yaml by default)
iii
```

Other useful subcommands: `iii worker add <name>` (install a worker from the registry), `iii trigger <function> <payload>` (invoke a function against the running engine), `iii console` (launch web console), `iii update` (update iii and managed binaries).

Install an SDK:

- Rust: `cargo add iii-sdk`
- Node (backend): `npm install iii-sdk`
- Node (browser, port 49135, RBAC-scoped): `npm install iii-browser-sdk`
- Python: `pip install iii-sdk`

Full docs: https://iii.dev/docs

## Ports

- `49134` — engine WebSocket (backend SDK connections)
- `49135` — browser WebSocket (RBAC-scoped, `iii-browser-sdk`)
- `3111` — REST API when the `iii-http` worker is loaded
- `3113` — console UI when `iii-console` is loaded

## Harness composition as a shape, not a product

The thin-vs-thick harness debate is a composition choice in iii. A thin harness is a worker with a few functions that lets the model decide what to trigger next. A thick harness is a worker with more functions, approval gates, and conditional logic before enqueuing the next step. Same primitives, different shape. Change the shape by adding or removing functions, not by rearchitecting.

## Process isolation

iii ships a sandbox worker that runs arbitrary ephemeral code on demand. Compose it with the RBAC worker to let agents run untrusted code without risk to the base system. The CLI uses the same sandbox functions when you run `iii worker add` with a sandbox target. An agent that needs to execute generated or installed code calls those same functions, gated by the same RBAC.

## Discovery and extensibility

The engine is the registry. It is always correct because it only reflects what is actually connected. No Consul, no service mesh, no OpenAPI specs drifting, no stale internal docs.

`iii worker add <name>` is the npm moment for connected systems. What gets installed is a running participant, not a library to integrate.

## Observability as protocol

OpenTelemetry traces, metrics, and structured logs come from the engine itself. A trace that starts at a browser click, flows through an agent, hits a Python ML worker, writes state, and renders back in the browser is one trace. Forward it to Datadog, Grafana, or Honeycomb. Stop writing instrumentation. Stop debugging across disconnected log streams.

## Memory and portability

Agent memory, traces, and function catalogs live wherever you run the engine. File-based for dev. Redis or Postgres for prod. Swap with a config change. No vendor has a copy.

## Licensing

Elastic-2.0. Source available. Free for direct use; restrictions on offering iii as a managed service.

## Links

- Homepage: https://iii.dev/
- Manifesto: https://iii.dev/manifesto
- Docs: https://iii.dev/docs
- llms.txt (AI discovery): https://iii.dev/llms.txt
- This file: https://iii.dev/AGENTS.md
- Monorepo AGENTS.md (build/test for contributors): https://github.com/iii-hq/iii/blob/main/AGENTS.md
- GitHub: https://github.com/iii-hq/iii

Last updated: 2026-04-29
