## What iii is, to an agent

An engine and a single open protocol (JSON over WebSocket) with three primitives: Function, Trigger, Worker. An agent is a worker. Its tools are functions. Its memory is state. Its orchestration is triggers.

No harness separate from the backend. No tool layer wired by hand. No memory system you integrate. One system, one trace, one set of primitives. The harness IS the system.

## The claim

MCP gives an agent a fixed tool menu prepared in advance. iii gives an agent a live registry of workers. When a worker connects, every function on it becomes discoverable and callable. When it disconnects, it unregisters. The agent does not browse a static catalog. It queries a live system that only contains what is actually connected right now.

An agent that hits a task outside its current capabilities can install a worker at runtime and use its functions immediately. Workers can create workers. For autonomous systems, this is the difference between scripted LLM calls and actual autonomy.

## Primitives (wire-level)

| Primitive | What it is                                                                                             | How an agent uses it                                                                                          |
| --------- | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------- |
| Worker    | A process that speaks the iii protocol and registers functions and triggers                            | Spawn via SDK; self-registers on connect                                                                      |
| Trigger   | What causes a function to run: direct call, HTTP, cron, queue subscription, state change, stream event | Declare on your worker; the engine handles routing and delivery                                               |
| Function  | Stable identifier (e.g. `orders::validate`) wrapping input → output                                    | Invoke from another worker via your language SDK against the engine (not the `iii trigger` CLI for app logic) |

## Install / start

For **current install paths and prerequisites**, use **[iii.dev/docs/installation](https://iii.dev/docs/installation)**—the snippets below may lag the docs.

```bash
curl -fsSL https://install.iii.dev/iii/main/install.sh | sh
iii
```

Engine **listeners, adapters, and port bindings** are defined in your project’s **`config.yaml`** (or the path you pass to the engine). Read that file and the docs; do not assume fixed port numbers from a static list.

Use **`iii console`** to launch the web observability console against the running engine.

Discover CLI surface area with **`iii --help`** and **`iii <subcommand> --help`**. The **`iii trigger`** subcommand is handy for **manual** invocations while debugging; it is **not** the primary way applications call functions—use the SDK from your workers for real integration, and **do not** build automation around the CLI trigger.

Other useful subcommands include `iii worker add <name>` (install a worker from the registry) and `iii update` (update iii and managed binaries).

Install an SDK:

- Rust: `cargo add iii-sdk`
- Node (backend): `npm install iii-sdk`
- Node (browser, RBAC-scoped): `npm install iii-browser-sdk`
- Python: `pip install iii-sdk`

Full docs: https://iii.dev/docs

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

Elastic-2.0.

## Links

- Homepage: https://iii.dev/
- Manifesto: https://iii.dev/manifesto
- Docs: https://iii.dev/docs
- llms.txt (AI discovery): https://iii.dev/llms.txt
- This file: https://iii.dev/AGENTS.md
- GitHub: https://github.com/iii-hq/iii
