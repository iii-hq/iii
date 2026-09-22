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

For **current install paths and prerequisites**, use **[iii.dev/docs/install](https://iii.dev/docs/install)**—the snippets below may lag the docs.

```bash
curl -fsSL https://install.iii.dev/iii/main/install.sh | sh   # answer "y" at the prompt to let the installer scaffold and start the harness
iii project init my-app          # barebones project; add `-t harness` for the harness template
cd my-app
iii compose --up                 # start the engine, the Compose daemon, and the project's workers
```

Add `--non-interactive` after `sh -s --` to install without the setup prompt. `iii project init --learn-iii` scaffolds the harness template and starts it in one step.

A project is one **`worker-compose.yaml`**: the `engine:` block declares the engine and its engine-owned workers, and `containers:` declares the project's workers (registry packages or local `path://` workers). Each worker's runtime settings (ports, hosts, adapters) live in the **configuration worker**, one YAML file per worker under `./config/`. Read those files and the docs; do not assume fixed port numbers from a static list. The list-shaped `config.yaml` is only for an engine that another supervisor owns.

The UI comes from the **console worker**. The project templates include it; once the project is up it serves at http://127.0.0.1:3113. Add it to another project with `iii trigger compose::add worker=console`.

Discover CLI surface area with **`iii --help`** and **`iii <subcommand> --help`**. **`iii trigger <function>`** is how a human or an agent outside iii calls a function on the running engine, including the `compose::*` lifecycle functions. Code that runs inside a worker calls functions through the SDK's `trigger`, not by shelling out to the CLI.

Add a worker from the registry with `iii trigger compose::add worker=<name>`; it writes the worker and its dependencies into `worker-compose.yaml` and restarts the project. `iii update` updates iii and its managed binaries. (`iii worker add` was removed in 0.23.)

Install an SDK:

- Rust: `cargo add iii-sdk`
- Node (backend): `npm install iii-sdk`
- Node (browser, RBAC-scoped): `npm install iii-browser-sdk`
- Python: `pip install iii-sdk`

Full docs: https://iii.dev/docs

## Guardrails

Agents should follow:

- Function IDs use `::` (e.g. `orders::validate`)
- HTTP `api_path` values use a leading slash (e.g. `/orders/validate`)
- Cron triggers use config field `expression`, not `cron`
- Inside a worker, call functions through the SDK's `trigger`; `iii trigger` is for humans and agents outside iii, including the `compose::*` lifecycle functions
- Project workers are declared in `worker-compose.yaml`; each worker's runtime settings live in `./config/<worker>.yaml`; the console worker provides the UI

## Agent skills (after onboarding)

Once iii is installed and your project is running, install the agent skills so your coding agent gets full iii context (primitives, SDKs, engine config, architecture patterns, error handling). Two sources, same commands:

```bash
npx skills add iii-hq/iii/skills   # all iii reference skills
npx skills add iii-hq/workers      # one skill per published worker
```

Neither source has a root skill, so a bare add discovers and installs every skill under it. Narrow to one by name or by path:

```bash
npx skills add iii-hq/iii/skills --skill <name>   # e.g. --skill iii-core-primitives
npx skills add iii-hq/workers --skill <worker>    # a single worker's skill
npx skills add iii-hq/workers/<worker>/skills     # the same, by path
```

Catalogs: https://github.com/iii-hq/iii/tree/main/skills and https://github.com/iii-hq/workers

## Harness composition as a shape, not a product

The thin-vs-thick harness debate is a composition choice in iii. A thin harness is a worker with a few functions that lets the model decide what to trigger next. A thick harness is a worker with more functions, approval gates, and conditional logic before enqueuing the next step. Same primitives, different shape. Change the shape by adding or removing functions, not by rearchitecting.

## Process isolation

iii ships a sandbox worker that runs arbitrary ephemeral code on demand. Compose it with the `rbac-proxy` worker (`iii trigger compose::add worker=rbac-proxy`), which puts role-based access control in front of the engine on its own port, to let agents run untrusted code without risk to the base system. Workers added with `compose::add` boot inside that same isolation. An agent that needs to execute generated or installed code calls those same functions, gated by the same RBAC.

## Discovery and extensibility

The engine is the registry. It is always correct because it only reflects what is actually connected. No Consul, no service mesh, no OpenAPI specs drifting, no stale internal docs.

`compose::add` is the npm moment for connected systems. What it declares is a running participant, not a library to integrate.

## Observability as protocol

OpenTelemetry traces, metrics, and structured logs come from the engine itself. A trace that starts at a browser click, flows through an agent, hits a Python ML worker, writes state, and renders back in the browser is one trace. Forward it to Datadog, Grafana, or Honeycomb. Stop writing instrumentation. Stop debugging across disconnected log streams.

## Memory and portability

Agent memory, traces, and function catalogs live wherever you run the engine. No vendor has a copy.

## Licensing

The iii engine is Elastic License 2.0 (ELv2). The SDKs, CLI, console, docs, and website are Apache License 2.0.

## Links

- Homepage: https://iii.dev/
- Manifesto: https://iii.dev/manifesto
- Docs: https://iii.dev/docs
- Blog index (markdown): https://iii.dev/blog/index.md
- llms.txt (AI discovery): https://iii.dev/llms.txt
- This file: https://iii.dev/AGENTS.md
- GitHub: https://github.com/iii-hq/iii
