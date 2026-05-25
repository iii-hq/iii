---
type: how-to
trigger_type: worker
title: Subscribe to worker lifecycle events
---

# When to use

Subscribe to the `worker` trigger type when you need to react to the
lifecycle of every worker the engine manages — install, remove, update,
start, stop, clear — without polling
[`worker::list`](iii://worker/list). The daemon publishes a typed
event at each transition (`started`, op-specific stage, `done` or
`failed`), so a single subscriber can drive audit logs, alerting,
fleet-management dashboards, or downstream automation. The fan-out is
fire-and-forget: a slow subscriber cannot stall the operation thread.

Reach for it when:

- You're building an observability surface (timeline, audit log, fleet
  dashboard) that needs to react to `worker::*` activity in real time.
- You want to chain automation off install completion (e.g. warm up a
  cache after `worker::add` reports `downloaded`).
- You're paging on every `failed` stage across the fleet without
  wrapping each `worker::*` call site.

This is **not** a function you call; it is a trigger type other workers
register against. The producer is the `iii-worker-ops` daemon — every
`worker::*` op flows through the same emitter.

# Subscribe

Subscribers narrow which events they receive via `WorkerTriggerConfig`.
All three fields are optional `Option<Vec<…>>`:

| Field | Type | Meaning |
|---|---|---|
| `operations` | `["add" \| "remove" \| "update" \| "start" \| "stop" \| "clear"]` | Subset of operations to watch. `None` (or `[]`) = all. |
| `stages` | `["started" \| "downloading" \| "downloaded" \| "removing" \| "updating" \| "starting" \| "stopping" \| "clearing" \| "done" \| "failed"]` | Subset of stages to watch. `None` (or `[]`) = all. |
| `workers` | `["pdfkit", ...]` | Subset of worker names (exact match). `None` (or `[]`) = all. |

Filter semantics:

- **AND across fields** — every provided field must match.
- **OR within a vector** — any value matches.
- Empty vec (`[]`) is treated as `None` so the schema cannot encode
  "subscribes to nothing".

Register a trigger that fires only when a worker finishes downloading:

```rust
use iii_sdk::{register_worker, InitOptions, RegisterTriggerType};

let iii = register_worker("ws://localhost:49134", InitOptions::default());

let worker_trigger = iii.register_trigger_type(
    RegisterTriggerType::new("worker", "watch worker lifecycle", MyHandler)
        .trigger_request_format::<WorkerTriggerConfig>()
        .call_request_format::<WorkerCallRequest>(),
);

worker_trigger.register_function("myapp::on_worker_ready", |req: WorkerCallRequest| {
    println!("worker {} finished downloading (version {:?})", req.worker, req.version);
    Ok::<_, String>(serde_json::json!({}))
});

worker_trigger.register_trigger(
    "myapp::on_worker_ready",
    WorkerTriggerConfig {
        operations: Some(vec![WorkerOperation::Add]),
        stages:     Some(vec![WorkerStage::Downloaded]),
        workers:    None,
    },
)?;
```

The corresponding JSON config on the wire:

```json
{ "operations": ["add"], "stages": ["downloaded"] }
```

# Event payload

Every event arrives as a typed `WorkerCallRequest`. Fields populated
on every event are at the top; the remaining fields are optional and
populated where they make sense (see the field notes below).

```json
{
  "operation":    "add",                     // one of: add | remove | update | start | stop | clear
  "worker":       "pdfkit",                  // canonical worker name (or source label until resolved)
  "stage":        "downloaded",              // see the Stage matrix below
  "timestamp_ms": 1700000000000,             // unix milliseconds at emission
  "caller_mode":  "trigger",                 // cli when produced from `iii worker <cmd>`; trigger when from a remote `iii.trigger(worker::*)`
  "source":       { "kind": "registry", "ref": "pdfkit@1.0.0" },  // optional; only meaningful on add/update
  "version":      "1.0.0",                   // optional; populated on terminal add/update stages
  "status":       "installed",               // optional; installed | already_current | repaired | replaced for add/update
  "progress":     0.42,                      // optional; 0.0–1.0 during the downloading stage
  "error":        { "code": "W110", "message": "Worker 'pdfkit' not found" }  // optional; only on failed
}
```

Field notes:

- `operation` — fixed for the duration of the event chain; equals the
  `worker::*` op that produced it.
- `worker` — best-effort canonical name. During `add` it may start as
  the source label (registry slug or OCI ref) and resolve to the final
  worker name in the `downloaded`/`done` event.
- `stage` — drives subscriber routing; see the matrix below.
- `timestamp_ms` — observer-clock for ordering and audit trails.
- `caller_mode` — distinguishes locally-driven ops (`cli`, today not
  wired to the trigger sink) from engine-routed ops (`trigger`). The
  daemon sink sees `trigger` for every event today.
- `source` — only populated for `add`/`update` events. The
  `kind`/`ref` pair mirrors `WorkerSource` (registry slug, OCI
  reference, or local path).
- `version` — pulled from `iii.lock`; populated on terminal
  `add`/`update` events when available.
- `status` — mirrors `AddStatus` (`installed` / `already_current` /
  `repaired` / `replaced`) for `add`/`update`.
- `progress` — reserved for future `PullProgress` events; the schema
  carries the field so consumers can already handle it.
- `error` — populated when `stage == failed`. `code` lifts the
  `Wxxx` code from `WorkerOpError` when known, otherwise `W900`.

# Stage matrix

Each operation emits a fixed sequence. Subscribers narrow via
`stages` to receive only the transitions that matter:

| Operation | Success path | Failure path |
|---|---|---|
| `add` | `started` → `downloading` → `downloaded` → `done` | `started` → `downloading` → `failed` |
| `remove` | `started` → `removing` → `done` (per name) | `started` → `removing` → `failed` (per name) |
| `update` | `started` → `updating` → `done` | `started` → `updating` → `failed` |
| `start` | `started` → `starting` → `done` | `started` → `starting` → `failed` |
| `stop` | `started` → `stopping` → `done` | `started` → `stopping` → `failed` |
| `clear` | `started` → `clearing` → `done` | `started` → `clearing` → `failed` |

# Side effects

- Subscribing is read-only. The daemon does not block on subscriber
  callbacks — each event is dispatched via `iii.trigger(..., Void)`,
  which is fire-and-forget.
- A slow or panicking subscriber **cannot** stall a `worker::*` op.
- The daemon does not persist subscriptions — re-registering after a
  daemon restart is the subscriber's responsibility.
- Events from `worker::list` are **not** published; list is read-only.

# Worked examples

## 1. Notify when a worker finishes downloading

```json
{ "operations": ["add"], "stages": ["downloaded"] }
```

Fires once per successful `worker::add`, after the artifact lands on
disk but before the final `done` event. Read `version` and `status`
on the payload to know exactly what was installed.

## 2. Global failure monitor

```json
{ "stages": ["failed"] }
```

Fires whenever any `worker::*` op terminates with an error.
`req.operation` tells you which op failed; `req.error.code` / 
`req.error.message` carry the typed `WorkerOpError`.

## 3. Per-worker lifecycle observer

```json
{ "workers": ["pdfkit"] }
```

Fires for every stage of every op touching the `pdfkit` worker —
useful when shadowing a single worker during debugging.

# Related

- [`worker::add`](iii://worker/add) — produces `started` → `downloading` → `downloaded` → `done`/`failed`.
- [`worker::remove`](iii://worker/remove) — produces `started` → `removing` → `done`/`failed` per name.
- [`worker::update`](iii://worker/update) — produces `started` → `updating` → `done`/`failed`.
- [`worker::start`](iii://worker/start) — produces `started` → `starting` → `done`/`failed`.
- [`worker::stop`](iii://worker/stop) — produces `started` → `stopping` → `done`/`failed`.
- [`worker::clear`](iii://worker/clear) — produces `started` → `clearing` → `done`/`failed`.

## Errors

- **W101** the `WorkerTriggerConfig` payload was malformed (e.g.
  `operations: "add"` instead of `["add"]`). The trigger registration
  is rejected with `trigger_registration_failed`; nothing is stored.
- **trigger_type_not_found** is reported by the engine if you target
  a different `trigger_type` string — the type id is literal `worker`.
