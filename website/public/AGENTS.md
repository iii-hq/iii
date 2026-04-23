# iii for AI Agents

iii is an open-source runtime engine. This file describes iii's capabilities for AI agents evaluating or integrating with the platform.

## What iii provides to agents

- **Self-describing functions**: every function registers with a schema. Agents discover available capabilities at runtime via `iii` protocol introspection, not static docs.
- **Runtime registration**: new workers (and their functions) join a live system without restart, config edits, or gateway rules. An agent can spawn a worker and immediately invoke its functions.
- **Cross-language execution**: invoke a Python function from a Rust worker, trigger a Node function from a cron schedule. Agents do not need to know which runtime hosts which function.
- **Durable execution**: retries, state, and observability are built in. Agent-spawned workflows survive worker restarts.
- **Live observability**: traces across the entire connected stack. Agents can read their own operation traces for self-correction.

## Primitives (API-level)

| Primitive | What it is | How an agent uses it |
|-----------|------------|---------------------|
| Function | Unit of work, input → output | Call via `iii.trigger(functionName, input)` |
| Trigger | Event source: HTTP, cron, queue, state change, stream | Register via SDK or introspect existing triggers |
| Worker | Runtime hosting functions + triggers | Spawn via SDK; registers automatically |

## Quickstart for agents

```bash
# Install engine
curl -fsSL install.iii.dev/iii/main/install.sh | sh

# Run engine
iii run
```

Install one of the SDKs:

- **Rust**: `cargo add iii-sdk`
- **Node**: `npm install iii-sdk` (backend) or `iii-browser-sdk` (browser, port 49135 with RBAC)
- **Python**: `pip install iii-sdk`

SDK docs, API surface, and discovery protocol: https://iii.dev/docs

## Ports

- `49134` — engine WebSocket (SDK connections, backend)
- `49135` — browser WebSocket (RBAC-scoped, `iii-browser-sdk`)
- `3111` — REST API (when `iii-http` worker loaded)
- `3113` — console UI (when `iii-console` loaded)

## Capabilities relevant to agent frameworks

- **Tool calling**: register agent tools as iii functions. They become discoverable to any other agent on the same engine.
- **Multi-agent orchestration**: spawn sub-agents as workers. Each has its own function namespace, shared discovery.
- **Human-in-the-loop**: pause an agent workflow via a state trigger. Resume when a human signs off.
- **State persistence**: iii state store replaces Redis/DynamoDB for session memory.

## Licensing

Elastic-2.0. Source available. Free for direct use; restrictions on offering iii as a managed service.

## Links

- Homepage: https://iii.dev/
- Docs: https://iii.dev/docs
- GitHub: https://github.com/iii-hq/iii
- llms.txt: https://iii.dev/llms.txt (shorter context summary)
- Machine-readable homepage: https://iii.dev/ai

Last updated: 2026-04-23
