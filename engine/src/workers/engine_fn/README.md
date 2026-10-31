# iii

iii is a language-agnostic runtime where services, agents, and tools are composed of the same things: workers, functions, and observers. One engine process (default port `49134`) holds a live registry of every connected worker, every function those workers expose, and every observer between those functions. Workers are independent processes that open a WebSocket to the engine, register **Functions** (`service::name` handlers), and **observe** functions: each matching call of the observed function invokes a target function. There is no direct worker-to-worker traffic — every call routes through the engine.

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
| Observer | A `(source function, filters, target function)` binding: each matching call of the source invokes the target. |

Every call is `caller → engine → handler`. The function id is the only contract between any two workers.

## Functions — `engine::*`

Implemented in-process by mandatory worker **`iii-engine-functions`**. Filter lists with `prefix`, `search`, or `worker`. By default, `engine::functions::list` hides internal `engine::*` rows unless `include_internal: true`.

For exact request/response JSON Schemas, call `engine::functions::info { function_id: "engine::…" }`.

| Function | Description |
|---|---|
| `engine::channels::create` | Create a streaming channel pair (writer + reader refs). |
| `engine::functions::list` | List registered functions with their owning worker name. |
| `engine::functions::info` | Inspect one function: schemas, owner, `args_format`, `private`, `observable`, and its observer count. |
| `engine::workers::list` | List workers connected to the engine (WebSocket-connected only). |
| `engine::workers::info` | Inspect one connected worker's full surface (functions, and the ids of the observers it created). |
| `engine::workers::register` | Register worker metadata (called by SDK workers on connect). |
| `engine::observers::list` | List observers: function, args, filters, target, observing worker, state, reason, fires, and drops by reason. |
| `engine::observers::get` | Get one observer by id. |
| `engine::observers::dispose` | Dispose an observer by id (its creator, or a caller no RBAC restricts). |

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
iii call -n dev compose::add worker=state
iii call -n dev compose::status file=worker-compose.yaml
iii call -n dev compose::logs file=worker-compose.yaml worker=state tail=100
iii call -n dev compose::restart file=worker-compose.yaml worker=state
iii call -n dev compose::update file=worker-compose.yaml worker=state
iii call -n dev compose::down file=worker-compose.yaml
```

`iii worker` and `worker::*` no longer exist. Use `engine::workers::list` for live registrations and
`compose::status` for process ownership and supervisor state. Use `compose::logs` for bounded raw
stdout and stderr, or `iii compose logs state --follow --namespace dev` for a live view.

## Engine-hosted sources

Owned by `iii-engine-functions`. Each is a private function: only the engine calls it, once per change, with the event as its input and output. Observe it like any function to react to the change.

| Function | Called when |
|---|---|
| `engine::on-functions-changed` | Functions are registered or unregistered, driven by the registry itself (no polling); a burst is coalesced over ~100ms into one call. Event: `{ event: "functions_changed", functions: [...] }`. |
| `engine::on-workers-changed` | A worker connects, disconnects, or updates its metadata through `engine::workers::register`. Event: `{ event, worker_id }`. |
| `engine::on-observers-changed` | An observer is created (with the state it starts in) or changes state. Event: `{ observer_id, function_id, target, state, reason? }`. |

## Before you build

Run these reads before authoring a new worker — the capability you need may already exist:

1. `engine::functions::list` — every function on this engine (filter with `prefix` or `search`).
2. `directory::registry::workers::list` — every worker in the public registry. Prefer `compose::add { worker: "…" }` over re-implementing.

Only hand-author a worker when both come up empty.

## Functions to observe (other workers)

Observe these functions to react to the events of other workers. Provider READMEs on the registry are authoritative when versions drift.

| Function | Provider | Args or filters |
|---|---|---|
| `http::on-request` | `http` | args `{ method, path }` |
| `cron::on-tick` | `cron` | args `{ expression }` — 7 fields: sec min hr dom mon dow year |
| `queue::on-message` | `queue` | args `{ topic, max_retries?, backoff_ms? }` |
| `state::on-changed` | `state` | filters on `input.scope`, `input.key` |
| `stream::on-changed` | `iii-stream` | filters on `input.stream_name`, `input.group_id` |
