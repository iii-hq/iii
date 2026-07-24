---
name: iii
type: index
description: What iii is and what it is capable of - the engine, the three primitives (Worker, Function, Trigger), how any capability is added and operated, and where every deeper reference lives.
---

# iii

iii organizes every category of software into three primitives on one runtime: **Workers** host work, **Functions** are the work, **Triggers** cause the work to run, and the **Engine** routes between them. Queues, cron, streaming, sandboxing, observability, agents, business logic, devices, and browser UIs are all built from these three things. Adding a capability is always the same operation: add a worker. Agents are workers too; their tools are functions, their memory is state, their orchestration is triggers, and an agent can extend the running system by registering new workers and functions at runtime.

How a call flows: every worker opens a WebSocket to one engine process (default port 49134). The engine keeps a live registry of every connected worker and every function and trigger they registered, and routes each invocation to whichever worker currently provides the target function. There is no direct worker-to-worker traffic, so language, runtime, and location are invisible to callers. Function ids follow `service::name` and stay stable across restarts; worker crashes drop only that worker's functions out of the routing table. Triggers bind three parts (a `type` like `http` or `cron`, a per-type `config`, and the `function_id` to invoke); one function can answer to any number of triggers, and every registered function is directly callable with no trigger at all.

## Extending

If a capability is missing, the gap is filled by a worker, never by changing the engine. Find one first: browse the public registry in-mesh with `directory::registry::workers::list` / `::info`, then install it by calling the `worker::add` function with `{ source: { kind: "registry", name: "<name>" } }` (a human at a terminal uses `iii worker add <name>` for the same thing). Writing a worker is a small contract in any language that speaks WebSocket and JSON. Before writing worker code, fetch the SDK reference page for your language as markdown; https://iii.dev/docs/llms.txt is the generated index of every doc page (Node.js, Python, Rust, browser SDKs, and the raw engine wire protocol). Do not write SDK code from memory.

## Operating

Bare `iii` starts the engine from `./config.yaml`; the file is watched, and only added, removed, or changed workers restart on an edit. Per-worker settings are a separate layer owned by the configuration worker: seeded from `config.yaml` once at first boot, then edited live from disk, `configuration::set`, or the console Workers tab with no engine reload. `iii console` is the visual UI (live traces, workers, functions, settings forms); `iii trigger <fn> key=value` invokes any registered function from the terminal. The docs describe the latest release line; on an older engine, read the upgrading guide before applying release-specific instructions.

## Where the depth lives

Live discovery beats documentation: `engine::functions::list` / `engine::functions::info` are the always-accurate source for what is callable right now, and `directory::skills::index` maps every connected worker's purpose, with `directory::skills::get { id }` serving each worker's full guide. API signatures and per-topic guides live in the docs, fetched per page through the llms.txt index above. Curated authoring skills (getting started, core primitives, SDK reference, engine config, architecture patterns, error handling) ship in the iii repo under `skills/` for coding agents via `npx skills add iii-hq/iii/skills`.
