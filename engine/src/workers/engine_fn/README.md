# iii

iii is a language-agnostic runtime where services, agents, and tools are composed of the same things: workers, triggers, and functions. One engine process (default port `49134`) holds a live registry of every connected worker, every function those workers expose, and every trigger bound to them. Workers are independent processes that open a WebSocket to the engine and register **Functions** (`service::name` handlers) and **Triggers** (events that invoke those functions). There is no direct worker-to-worker traffic — every call routes through the engine.

This registry worker documents the in-process **`engine::*`** introspection surface. It is always present in the engine and is not configured in `config.yaml`.

This worker is built into the engine and is always available; no install step is needed.

## Skills

Install the `iii` agent skill for Claude Code, Cursor, and 30+ other agents:

```bash
npx skills add iii-hq/iii --full-depth --skill iii
```

## Registry name vs runtime name

| Surface | Name | Notes |
|---|---|---|
| Registry / manifest | **`iii`** | [`iii.worker.yaml`](iii.worker.yaml) `name:` field |
| Engine runtime worker (in-process) | **`iii-engine-functions`** | Mandatory builtin; owns `engine::*` |
| `engine::functions::list` owner for `engine::*` | **`iii-engine-functions`** | Until runtime name aligns with registry |

Use `engine::workers::info { name: "iii-engine-functions" }` (not `"iii"`) for full introspection of the in-process surface today. Filter with `engine::functions::list { prefix: "engine::", include_internal: true }` to list `engine::*` handlers without needing the worker name.

## Architecture

| Primitive | What it is |
|---|---|
| Engine | One coordinator process. Routes every invocation. |
| Worker | A process that opens a WebSocket to the engine. |
| Function | A named handler inside a worker, id `service::name`. |
| Trigger | A `(type, config, function_id)` triple that causes a function to run when an event fires. |

Every call is `caller → engine → handler`. The function id is the only contract between any two workers.

## Functions — `engine::*`

Implemented in-process by mandatory worker **`iii-engine-functions`**. Filter lists with `prefix`, `search`, or `worker`. By default, `engine::functions::list`, `engine::triggers::list`, and `engine::registered-triggers::list` hide internal `engine::*` rows unless `include_internal: true`.

For exact request/response JSON Schemas, call `engine::functions::info { function_id: "engine::…" }`.

| Function | Description |
|---|---|
| `engine::channels::create` | Create a streaming channel pair (writer + reader refs). |
| `engine::functions::list` | List registered functions with their owning worker name. |
| `engine::functions::info` | Inspect one function: schemas, owner, and registered triggers. |
| `engine::triggers::list` | List trigger types registered with the engine. |
| `engine::triggers::info` | Inspect one trigger type: schemas, owner, and live instance count. |
| `engine::registered-triggers::list` | List registered trigger instances (subscriber rows). |
| `engine::registered-triggers::info` | Inspect one registered trigger with denormalized trigger and function detail. |
| `engine::workers::list` | List workers connected to the engine (WebSocket-connected only). |
| `engine::workers::info` | Inspect one connected worker's full surface (functions, trigger types, registered triggers). |
| `engine::workers::register` | Register worker metadata (called by SDK workers on connect). |

### `engine::functions::list` filters

| Field | Type | Description |
|---|---|---|
| `prefix` | string | Exact prefix match on `function_id`. |
| `search` | string | Case-insensitive substring on `function_id` and `description`. |
| `worker` | string | Exact worker-name match. |
| `include_internal` | boolean | Include `engine::*` rows. Defaults to `false`. |

## Worker lifecycle — `compose::*`

Project workers are declared in `worker-compose.yaml` and supervised by a Compose daemon. Address
that daemon with its namespace:

```bash
iii trigger -n dev compose::add worker=state
iii trigger -n dev compose::status file=worker-compose.yaml
iii trigger -n dev compose::logs file=worker-compose.yaml worker=state tail=100
iii trigger -n dev compose::restart file=worker-compose.yaml worker=state
iii trigger -n dev compose::update file=worker-compose.yaml worker=state
iii trigger -n dev compose::down file=worker-compose.yaml
```

`iii worker` and `worker::*` no longer exist. Use `engine::workers::list` for live registrations and
`compose::status` for process ownership and supervisor state. Use `compose::logs` for bounded raw
stdout and stderr, or `iii compose logs state --follow --namespace dev` for a live view.

## Trigger types

### Reactive — owned by `iii-engine-functions`

| Trigger type | Description |
|---|---|
| `engine::functions-available` | Fires when the function registry changes (polled every 5s). Payload includes `event: "functions_changed"` and the current function list. |
| `engine::workers-available` | Fires when worker metadata is updated via `engine::workers::register`. |

## Before you build

Run these reads before authoring a new worker — the capability you need may already exist:

1. `engine::functions::list` — every function on this engine (filter with `prefix` or `search`).
2. `directory::registry::workers::list` — every worker in the public registry. Prefer `compose::add { worker: "…" }` over re-implementing.

Only hand-author a worker when both come up empty.

## Canonical trigger types (other workers)

Pass these literal strings as `type` in `registerTrigger`. Provider READMEs on the registry are authoritative when versions drift.

| `type` | Provider | Config keys |
|---|---|---|
| `http` | `http` | `{ http_method, api_path }` |
| `cron` | `cron` | `{ expression }` — 7 fields: sec min hr dom mon dow year |
| `queue` | `queue` | `{ queue, retries }` |
| `state` | `state` | `{ scope, condition_function_id }` |
| `stream` | `iii-stream` | `{ stream_name }` |
