# Transition: `workers/database` to `database::on-row-changed`

How to move the database worker from the `database::row-changed` trigger type
to the observable function designed in [`database.md`](database.md), and what
was checked before writing it down. Read `database.md` first: this document
does not repeat the design, it records what the 0.25 engine and SDKs do with
it, what had to change, and the order to ship it in.

## 0. Status

- **Validated, not migrated.** Both halves were run on 2026-10-16 against the
  iii repo's `feat/observe` tree (engine and Rust SDK 0.25, versioned
  `0.24.6-rc.5` in the tree):
  - test A: the current worker, unchanged, on the 0.25 SDK and engine (§3);
  - test B: a prototype of the migration (§4), with its own tests (§6).
- **Prototype branch:** `observe/database-transition` in
  `/home/sergio/Documents/workspaces/iii/workers`, local only, never pushed.
  It was checked out as a worktree under the session scratchpad; if that
  directory is gone, `git worktree prune` and check the branch out anywhere.
  Commits, oldest first:

  | Commit | What |
  |---|---|
  | `99a72f91c` | `[patch.crates-io]` to the local SDK (not for merge) |
  | `ba990e170` | the prototype as `database.md` §2.3 writes it (routed through `iii.trigger`) |
  | `1ca847849` | in-process routing; the source refuses invocations (§4.1) |
  | `e6f0f573f` | test: the shim carries the trigger metadata sidecar |
  | `0f952e164` | legacy bindings stay filed across a config reload |
  | `82744c607` | wording |
  | `ea65da934` | `tests/e2e/run-local.sh`: the e2e harness against any engine binary, no docker |

- **Base:** workers `origin/main` at `6d8b34175` (`iii-sdk = "0.24.5"`). The
  local `main` (`0372d295f`) still pins `0.23`; `database/src` is identical in
  both.
- **Not covered:** MySQL (no server was available; `mysql_db` cases did not
  run), the console UI subscription (`ui/src/page/useRowChanges.ts`), harness
  bindings end to end, and the 5k statements/s benchmark.
- **Fixed in 0.25 after the prototype ran** (§8 R1 to R3):
  - the engine refuses an invocation of an owner-delivered function from any
    connection but the one hosting it, with `FORBIDDEN` ("`<id>` is
    owner-delivered: only the worker that hosts it invokes it; observe it
    instead");
  - `engine::registered-triggers::list` / `::info` list each binding the shim
    serves from the moment it is stored: `status: active` while the stand-in
    function is registered (`pending`, shown with `include_pending`, while it
    is away), `observer_state` (`pending` until the owner acks, then
    `active`), and `served_by` (the stand-in); a binding the owner rejected is
    not listed. `engine::triggers::list` keeps listing the trigger type, as
    served by the stand-in;
  - the Rust SDK has `IIIClient::engine_supports_observe() -> Option<bool>`
    and `on_engine_supports_observe(listener) -> ObserveSupportListener`, and
    warns about inactive hooks for a function registered after the engine's
    features are known.

  Re-run on the fixed tree: the e2e harness with its original readiness check
  (only the catalog list changed) passes 70/70 on SQLite, and
  `tests/observe.rs` 9/9.
- **`database.md` is updated** with what the prototype found (§2.1 to §2.5:
  in-process routing, `Deserialize`, what canonical args cannot say, the
  install refcount key, the legacy event shape).

## 1. What changes, in one paragraph

The worker stops registering a trigger type and keeping its own binding table.
It registers `database::on-row-changed`, owner-delivered, with
`args_format = RowChangedArgs` (the old trigger config) and
`legacy_trigger_type = "database::row-changed"`. `NativeCapture`, its
`ObserveHandler`, validates each observer, installs native capture, and files
the observer in a `(db, table)` index. Every committed write statement is
routed through that index to `obs.emit`, in-process, with one atomic load when
nobody observes. Old consumers keep working: on a 0.25 engine through the
engine's legacy shim, on an older engine through the old trigger type, which
the worker registers only there (§5.2).

## 2. Prerequisites

- **Published crates, in this order:** `iii-filter` 0.25.0 (new; `iii-sdk`
  depends on it), `iii-helpers` 0.25.0, `iii-sdk` 0.25.0. The engine 0.25.0
  for the new path; nothing else is required of the engine (§5.2).
- **Workers repo:** `crates/console-ui/Cargo.toml` pins
  `iii-sdk = ">=0.24.5, <0.25.0"`, so no worker that links it can take 0.25.
  Widen it to `<0.26.0` in the SDK bump (prototype: `99a72f91c`).
- **Feature negotiation** ([`protocol.md`](protocol.md) §1). Against an engine
  without `observe.v1` the SDK registers `database::on-row-changed` as a plain
  function: no `functionregistered`, no hooks, no shim, and `observe()` fails
  with `UNSUPPORTED`. Old consumers would hear nothing, so the worker must keep
  the old trigger type for that engine. The 0.25 SDK says which engine it is
  on: `IIIClient::engine_supports_observe()` (`None` until `WorkerRegistered`
  arrives and while reconnecting), and `on_engine_supports_observe(listener)`
  for the value once known and every later change. The prototype predates it
  and asks the engine instead: `engine::observers::list` exists only in 0.25,
  and `function_not_found` means an older engine (§4.6, §5.2, §8 R1).

## 3. Test A: compatibility

### 3.1 Matrix

"0.24 engine" is the installed release `~/.local/bin/iii` 0.24.6-rc.5 (no
`observe.v1`); "0.25 engine" is `cargo build -p iii` from `feat/observe`.
"Compat" is the current worker built against the 0.25 SDK with no code change
(only the patch). e2e counts are the harness's own cases for one driver
(`no-bypass` mode: 68; `full` mode adds the bypass repros: 70 SQLite, 71
Postgres).

| Worker | Engine | Unit + integration | e2e SQLite | e2e Postgres |
|---|---|---|---|---|
| 0.24.5 (published SDK) | 0.24 | | 68/68 | 68/68 |
| 0.24.5 | 0.25 | | 68/68 | 68/68 |
| compat (0.25 SDK) | 0.25 | 374 + 1 + 28 + 3 pass | 68/68 | 68/68 |
| compat | 0.24 | | 68/68 | 68/68 |
| prototype (§4) | 0.25 | 374 + 1 + 28 + 3 pass, `observe.rs` 9/9 | 70/70 (`full`) | 71/71 (`full`) |
| prototype | 0.24 | | 70/70 (`full`) | 71/71 (`full`) |

What the rows show:

- **Old worker, new engine.** Unchanged. The worker registers
  `database::row-changed` through the old path; consumers'
  `registerTrigger({ type: 'database::row-changed' })` fire as before. No new
  engine warnings compared with the 0.24 baseline.
- **Compat worker (0.25 SDK), either engine.** Unchanged behavior. The
  deprecations show up as expected:
  - compile: 3 warnings, one per call site: `src/main.rs:690`
    (`register_trigger_type`), `src/configuration.rs:175` (`register_trigger`
    of the `configuration` trigger), `crates/console-ui/src/lib.rs:430`
    (`register_trigger` of `console:script` / `console:style`);
  - runtime: one `register_trigger_type is deprecated` and one
    `register_trigger is deprecated` warning per process, although
    `register_trigger` is called three times.
- **New SDK, old engine.** Every call is routed, nothing is pushed; the worker
  behaves as with 0.24.5.
- **Prototype.** Both engines, legacy consumers only (the harness is a Node
  0.24.5 consumer): on 0.25 they go through the shim, on 0.24 through the
  fallback trigger type. Compile warnings: 1 (console-ui). Runtime warnings:
  `register_trigger` once on 0.25 (console-ui), plus `register_trigger_type`
  once on 0.24 (the fallback).

Two harness changes were needed for the prototype rows, both in the branch:
the catalog guard lists `database::on-row-changed`, and the cases' "binding is
visible to the engine" check also reads `engine::observers::list`, because the
shim's observers were not in `engine::registered-triggers::list` (§8 R3).
Without the second change 5 cases failed on that tree at that check, not at
delivery. The fixed 0.25 engine lists them, and the harness passes with its
original check (70/70 on SQLite): only the catalog list change remains.

### 3.2 Commands

```sh
# worktree and builds (target dirs on disk; the UI bundle copied from the main checkout)
git -C ~/Documents/workspaces/iii/workers worktree add -b observe/database-transition <dir> origin/main
SKIP_UI_BUILD=1 CARGO_TARGET_DIR=~/Documents/workspaces/iii/iii/target/workers-database-024 \
  cargo build --locked --bin database --tests            # 0.24.5, before the patch
SKIP_UI_BUILD=1 CARGO_TARGET_DIR=~/Documents/workspaces/iii/iii/target/workers-database \
  cargo build --bin database --tests && cargo test       # 0.25 SDK, after the patch
(cd ~/Documents/workspaces/iii/iii && cargo build -p iii)  # copied out before use

# Postgres without docker: a local server over its unix socket
psql -d postgres -c 'CREATE DATABASE iii_observe_e2e'
export TEST_POSTGRES_URL='postgresql://sergio@%2Fvar%2Frun%2Fpostgresql/iii_observe_e2e'
cargo test                              # the postgres-gated unit tests now run too

# e2e: <engine-bin> <database-bin> <sqlite_db|pg_db|mysql_db> <full|no-bypass>
PORT=49360 database/tests/e2e/run-local.sh <engine> <worker> sqlite_db full

# live-engine tests of the prototype (an engine listening on 49360)
III_TEST_ENGINE_URL=ws://127.0.0.1:49360 cargo test --test observe -- --nocapture
```

`run-local.sh` does what `run-tests.sh` steps 6 to 14 do, on a port of your
choice and a fresh configuration directory, without docker.

## 4. The migration, file by file

Paths are in `workers/database`. Code is from the prototype, trimmed.

### 4.1 Route in-process (now in `database.md` §2.3)

The first draft of `database.md` §2.3 called the function locally
(`iii.trigger(TriggerRequest::new(ROW_CHANGED_FN, payload))`) and let its
handler route. That works (commit `ba990e170` passed everything below), but on
the tree the prototype ran against the function was an ordinary function to
the engine: any worker allowed to invoke `database::on-row-changed` could hand
the router a forged event, every routed observer received it, and since the
engine stamps a report's `caller.worker_*` with the reporting connection, the
event arrived attributed to the database worker (§8 R2). The 0.25 engine now
refuses an invocation of an owner-delivered function from any connection but
the one hosting it (`FORBIDDEN`).

The migration still routes from the bus, in-process, and the function's
handler still refuses every call:

- it needs no local dispatch: with `observers.local_dispatch: false`, or under
  RBAC middleware, a local call of the function turns into a round trip
  through the engine for every observed write;
- the refusing handler also covers an engine older than 0.25, which has no
  owner check;
- the observers see the same `caller`: emitting from inside the
  `database::execute` handler yields
  `{ worker_name: "database", function_id: "database::execute", parent: <the writer> }`,
  as the local call did.

One thing is lost: the function's local invocation counts in the engine's
metrics.

### 4.2 `Cargo.toml`

`iii-sdk = "0.25"`, `iii-helpers = "0.25"`. No new dependency.

### 4.3 `src/triggers/sql.rs`

Add the canonical rule (`database.md` §2.2) next to `same_table`, which now
compares canonical names:

```rust
/// The last dotted segment, unquoted, lowercased.
pub fn canonical_table(t: &str) -> String { bare_part(t.rsplit('.').next().unwrap_or(t)) }

/// `public."Orders"` -> (Some("public"), "orders")
pub fn split_table(t: &str) -> (Option<String>, String) { /* rsplitn(2, '.') + bare_part */ }
```

### 4.4 `src/triggers/bus.rs`

- `RowChangedConfig` becomes `RowChangedArgs` (same fields, still
  `deny_unknown_fields`); keep `pub type RowChangedConfig = RowChangedArgs`.
- `RowChangedEvent` gains `schema: Option<String>` (serialized as `null` when
  unknown), its `table` is canonical, and it derives `Deserialize` (it is the
  function's input type, so its `request_format` and the engine's filter path
  checks come from it).
- Add `LegacyRowChangedEvent` and `RowChangedEvent::legacy_view()`: the
  pre-0.25 shape, no `schema`, `table` schema-qualified when a schema is known.
  Legacy bindings receive this (§5.1).
- Add `RowChangedEvent::from_raw(db, raw_table, ...)` (statements path) and
  `canonical()` (native producers fill `table` with what the database said,
  `public.orders` on Postgres, and the bus splits it).
- Delete `Subscriber`, `SubscriberFilter`, `targets_for`, `register`,
  `unregister`, `subscriber_count` and their tests. Keep the transaction buffer
  (`stage` / `commit` / `rollback`) unchanged; staging stays unconditional.
- `RowChangeBus::with_capture(iii, capture)`; keep `new(iii, timeout)` for the
  tests that only exercise the buffer.

```rust
pub async fn emit(&self, db: &str, sql: &str, affected_rows: u64,
                  returning: Option<&[Map<String, Value>]>) {
    if affected_rows == 0 || !self.capture.is_listened() {
        return; // nobody listening: not even classified
    }
    let Some(mutation) = sql::classify(sql) else { return };
    self.fan_out(RowChangedEvent::from_raw(db, mutation.table.as_deref(), mutation.op,
                                           affected_rows, returning.map(<[_]>::to_vec), false)).await;
}

async fn fan_out(&self, event: RowChangedEvent) {
    if !self.capture.is_listened() {
        return; // no serialization, no call, no frames
    }
    // In-process, from the invocation that made the write; awaited, so a
    // transaction's statements are reported in commit order.
    self.capture.route(&event).await;
}
```

### 4.5 `src/triggers/capture.rs` (new; replaces `handler.rs`)

The function, its hooks and the index.

```rust
pub const ROW_CHANGED_FN: &str = "database::on-row-changed";

pub fn register(iii: &IIIClient, capture: &Arc<NativeCapture>) -> FunctionRef {
    let function = iii.register_function(
        ROW_CHANGED_FN,
        RegisterFunction::new(|_event: RowChangedEvent| {
            Err::<(), _>(Error::Remote { code: "FORBIDDEN".into(), message: "… is an event source: observe it".into(),
                                         stacktrace: None })
        })
        .description(ROW_CHANGED_DESCRIPTION)
        .observe(Observe::owner()
            .args_format::<RowChangedArgs>()
            .legacy_trigger_type(ROW_CHANGED_TYPE)
            .handler(capture.clone())),
    );
    capture.attach(function.clone()); // the hooks exist before the FunctionRef does
    function
}
```

- **`Sink`**: `Observer(Observer)` or `Trigger { function_id, metadata }` (the
  pre-0.25 fallback, §5.2). **`Route`**: `{ id, db, table (canonical), ops,
  sink, legacy, installed }`. **`RouteIndex`**: `(db, table) -> routes` plus an
  id map; `matching(db, table, op)` returns the routes on `(db, table)` and on
  `(db, None)` whose `ops` accept `op`; an event with no table reaches only the
  db-wide routes. A counter of `Trigger` sinks makes `is_listened()` two atomic
  loads: `function.is_observed() || trigger_sinks > 0`.
- **`route`** emits the canonical event to new observers and
  `legacy_view()` to legacy ones; `Ok(false)` from `emit` (filter miss, removed
  observer) needs no guard. Fallback sinks get a `void` trigger with their
  metadata, as today.

```rust
for route in self.index.matching(&event.db, event.table.as_deref(), event.op) {
    match &route.sink {
        Sink::Observer(obs) => {
            let sent = if route.legacy { obs.emit(&event.legacy_view()) } else { obs.emit(event) };
            if let Err(e) = sent { tracing::warn!(observer = %obs.id(), error = %e, "row-changed emit failed"); }
        }
        Sink::Trigger { function_id, metadata } => { /* iii.trigger(Void, legacy_view, metadata) */ }
    }
}
```

- **Hooks:**

```rust
async fn on_observer_added(&self, obs: &Observer) -> Result<(), ObserveRejection> {
    let (args, install, legacy) = match obs.legacy() {
        // The shim skips args_format for it: validate as the trigger handler did.
        Some(legacy) => { let (args, install) = Self::legacy_args(&legacy.config)?; (args, install, true) }
        None => {
            let args = obs.args::<RowChangedArgs>()?;   // db required, typos refused: by the engine
            Self::check_canonical(&args)?;              // "Orders" -> NON_CANONICAL_TABLE, hint "orders"
            let install = args.table.clone();
            (args, install, false)
        }
    };
    let installed = self.admit(&args, install.as_deref()).await?;  // UNKNOWN_DB, NATIVE_CAPTURE_REQUIRES_TABLE,
                                                                   // CAPTURE_INSTALL_FAILED
    self.file(obs.id().to_string(), args, Sink::Observer(obs.clone()), legacy, installed); // before the ack
    Ok(())
}

async fn on_observer_removed(&self, obs: &Observer) { self.unfile(obs.id()); }
```

- **`legacy_args`** parses the old config (`CONFIG_ERROR` on a typo, as before)
  and returns the args with the canonical table plus the table **as the binding
  spelled it**: native capture installs on that spelling, so a Postgres binding
  on `"ORDERS"` still fails its install, as today.
- **`admit`** checks the live config, then installs native capture once per
  `(db, table as installed)`. Key the refcount by that spelling, not by the
  canonical name: keyed by `orders`, a wrong-case legacy binding rode on the
  correct binding's install and was accepted (found by the Postgres e2e case
  "native capture table casing behaves per driver contract"). The install code
  (advisory lock, DDL, changelog, binlog checks) moves here unchanged from
  `handler.rs`; the DDL stays installed when the count reaches zero.
- **`revalidate`** (called by `apply_config`) sends `obs.reject(UNKNOWN_DB, ..)`
  to every new observer whose database is gone, and
  `NATIVE_CAPTURE_REQUIRES_TABLE` to a table-less one whose database became
  native. Legacy bindings stay filed, as before: rejecting one would remove it
  for good and its registrant would never hear why.

### 4.6 `src/triggers/legacy.rs` (new)

The pre-0.25 fallback, deleted once the minimum engine is 0.25 (§5.4). The
probe below is the prototype's; the 0.25 SDK's `engine_supports_observe()`
replaces it (end of this section).

```rust
pub async fn engine_supports_observe(iii: &IIIClient) -> bool {
    let probe = iii.trigger(TriggerRequest::new("engine::observers::list",
                                                json!({ "function_id": ROW_CHANGED_FN }))).await;
    !matches!(probe, Err(Error::Remote { ref code, .. }) if code == "function_not_found")
}

#[allow(deprecated)]
pub fn register_trigger_type(iii: &IIIClient, capture: Arc<NativeCapture>) -> TriggerTypeRef<..> {
    iii.register_trigger_type(RegisterTriggerType::new(ROW_CHANGED_TYPE, DESCRIPTION, LegacyRowChanged { capture })
        .trigger_request_format::<RowChangedArgs>()
        .call_request_format::<LegacyRowChangedEvent>())
}
```

`LegacyRowChanged`'s `register_trigger` is `legacy_args` + `admit` + `file`
with a `Sink::Trigger`; `unregister_trigger` is `unfile`. The old handler's
tests move here (the concurrent Postgres install test included).

With the 0.25 SDK, replace the probe with the client's own signal and follow
it: register the trigger type when the engine lacks observe, unregister it
when observe comes back. The 0.25 engine honors `unregisterTriggerType`, and
bindings left without a provider are taken by the stand-in
(`engine/src/engine/mod.rs`, `UnregisterTriggerType`). The worker then no
longer needs a restart when the engine version changes under it (§5.2).

```rust
let fallback: Arc<Mutex<Option<TriggerTypeRef<..>>>> = Default::default();
let _support = iii.on_engine_supports_observe({          // keep the listener for the process's life
    let (iii, capture, fallback) = (iii.clone(), capture.clone(), fallback.clone());
    move |observe| {                                       // off the read loop: may call the client
        let mut fallback = fallback.lock().unwrap();
        match (observe, fallback.is_some()) {
            (false, false) => *fallback = Some(legacy::register_trigger_type(&iii, capture.clone())),
            (true, true) => {
                fallback.take();
                #[allow(deprecated)]
                iii.unregister_trigger_type(ROW_CHANGED_TYPE);
            }
            _ => {}
        }
    }
});
```

The listener is called once the value is known (right away when it already
is) and on every later change, never for a reconnect to an engine that says the
same. The `configuration` reload trigger (§4.9) follows the same signal.

### 4.7 `src/triggers/handler.rs`

Delete. Its `pg_install_hint` and install code live in `capture.rs`, its tests
in `capture.rs` and `legacy.rs`.

### 4.8 Native producers

`native.rs`, `sqlite_watch.rs`, `mysql_binlog.rs`: add `schema: None` to the
`RowChangedEvent` literal. `RowChangeBus::emit_event` canonicalizes.

### 4.9 `src/configuration.rs`

- `apply_config`: after swapping pools and config (and dropping both guards),
  `bus.capture().revalidate().await`.
- The reload trigger: the engine 0.25 hosts `configuration::on-updated`;
  observe it, and keep the trigger only for an older engine.

```rust
if engine_observes {
    let _ = iii.observe(                                   // replayed on reconnect; no handle needed
        Source::function("configuration::on-updated").namespace("default")
            .filters(json!({ "input.id": { "$eq": config_id() },
                             "input.event_type": { "$eq": "configuration:updated" } }))
            .deliver(Deliver::Input),
        CONFIG_FN_ID,
    );
    return Ok(());
}
#[allow(deprecated)]
iii.register_trigger(RegisterTriggerInput::new("configuration", CONFIG_FN_ID, json!({ … })))?;
```

Checked on both engines: `configuration::set` reloads the pools.

### 4.10 `src/main.rs`

Build the shared config and pools first, then the capture over them, then the
bus; register the function where the trigger type was; probe; register the
fallback when needed.

```rust
let pools = Arc::new(RwLock::new(pools));
let live_config = Arc::new(RwLock::new(cfg.clone()));
let capture = Arc::new(NativeCapture::new(iii.clone(), live_config.clone(), pools.clone(),
                                          ROW_CHANGE_DISPATCH_TIMEOUT_MS));
let row_changes = Arc::new(RowChangeBus::with_capture(iii.clone(), capture.clone()));
// … NativeListeners, AppState { pools, config: live_config, row_changes: Some(row_changes), .. }, functions …

database::triggers::capture::register(&iii, &capture);
let engine_observes = database::triggers::legacy::engine_supports_observe(&iii).await;
let _row_changed_type = (!engine_observes)
    .then(|| database::triggers::legacy::register_trigger_type(&iii, capture.clone()));
configuration::register_config_trigger(&iii, state.clone(), Some(native_listeners.clone()), engine_observes)?;
```

### 4.11 Tests

- Unit: routing, idempotent filing, canonical names and hints, legacy configs,
  admission against the live config (`capture.rs`); the buffer and the event
  shapes (`bus.rs`); the moved trigger-handler tests (`legacy.rs`).
- `tests/observe.rs` (new): against a live engine, gated by
  `III_TEST_ENGINE_URL` like the Postgres tests are by `TEST_POSTGRES_URL`.
  Each test runs the worker's handlers and `capture::register` in-process, in a
  namespace of its own. §6 lists them.
- e2e harness: the two changes in §3.1. The cases' expectations do not
  change: legacy consumers see what they saw.

### 4.12 Not in the prototype

`README.md` (the `database::row-changed` section and the engine-managed
example), `skills/SKILL.md`, the worker description strings, and
`ui/src/page/useRowChanges.ts` (the console's live feed calls the browser SDK's
`registerTrigger`; it keeps working through the shim and moves to `observe`
when the console runs the 0.25 browser SDK). Write the docs against
`database.md` §3's examples.

## 5. Compatibility plan

### 5.1 Old consumers on a 0.25 engine: the shim

- `registerTrigger({ type: 'database::row-changed', config })` becomes an
  observer with `legacy = { trigger_type, config }` and `deliver: input`
  ([`protocol.md`](protocol.md) §6). Checked from a 0.24.5 Node SDK (the whole
  e2e harness) and from the 0.25 Rust SDK (`tests/observe.rs`).
- The target receives the event in its pre-0.25 shape: `legacy_view()`. That
  is what keeps `"table": "public.orders"` for Postgres native capture (the e2e
  native cases assert it) and keeps `schema` out of old payloads. `database.md`
  §2.5 says the target "receives the event as before"; that holds only because
  the owner emits a different value to legacy observers.
- The trigger's metadata is the activation's sidecar: a harness binding's
  `__binding` arrives (asserted).
- An `"Orders"` or `"public.orders"` binding joins the index as `orders` and
  matches what it matched before; native capture installs on the binding's own
  spelling, so per-driver casing behavior is unchanged.
- An owner restart (a deploy) replays both new and legacy observers to the new
  process (asserted).
- **What changes for old consumers:**
  - `engine::registered-triggers::list` lists the binding as soon as it is
    stored, with `status: active`, as before; it adds `served_by:
    "database::on-row-changed"` and `observer_state`, which stays `pending`
    until the owner's hook acked it (after the native install) and is
    `active` from then on: events reach the binding from that point;
  - a binding the owner rejects (unknown `db`, a native database without
    `table`, a failed install) is not listed, as a rejected trigger was not;
  - `engine::triggers::list` lists `database::row-changed` as served by
    `database::on-row-changed` ("Served by `database::on-row-changed`, which
    stands in for this trigger type (observe)"), with the old config as its
    configuration schema;
  - the binding also appears in `engine::observers::list`, with `legacy` set.

### 5.2 Old engines: the fallback

Against an engine without `observe.v1` (every 0.24.x), the worker registers
`database::row-changed` as before, over the same index (`Sink::Trigger`). The
decision is made once at startup by the probe in §4.6. Consequences:

| Situation | Result |
|---|---|
| started on 0.24, stays on 0.24 | old behavior; new consumers get `UNSUPPORTED` from `observe()` |
| started on 0.25, stays on 0.25 | new path; shim for old consumers; no trigger type |
| started on 0.24, engine upgraded underneath | with the probe: the SDK replays the trigger type; a live old-style provider wins over the shim, so legacy bindings keep the old path (they still work, both kinds share the index); new observers work, and the shim takes over at the next restart. With `on_engine_supports_observe`: the worker unregisters the type and the shim takes over at once |
| started on 0.25, engine downgraded underneath | with the probe: no trigger type, legacy bindings park as pending and hear nothing until the worker restarts. With `on_engine_supports_observe`: the worker registers the type and they come back |

The first two rows were run (§3.1); the last two follow from the SDK's replay
and the engine's provider precedence and were not run. With the probe, restart
the database worker after changing the engine version under it; with the 0.25
SDK's `on_engine_supports_observe` (§4.6) it follows on its own.

### 5.3 New consumers

`iii.observe({ function_id: 'database::on-row-changed', args: { db, table?, ops? } }, target)`
on a 0.25 engine, with any 0.25 SDK. `database.md` §3 has the examples. Args
must use the canonical table; the rejection names it.

### 5.4 Deprecation timeline

| Release | Worker | Consumers |
|---|---|---|
| iii 0.25 | (unchanged worker keeps working; 3 compile warnings) | `registerTrigger` warns once per process |
| database worker after the migration | function + shim on 0.25; trigger type on 0.24 | old and new both work |
| minimum engine raised to 0.25 | delete `legacy.rs` and the probe | `registerTrigger` through the shim only |
| phase 6 (trigger APIs removed) | drop `legacy_trigger_type`, `LegacyRowChangedEvent`, `legacy_args` | `observe` only; harness bindings must have moved |

## 6. Test plan

`database.md` §5, item by item:

| Acceptance item | Covered by | Status |
|---|---|---|
| existing suites pass on the legacy and the new path | unit + integration (374 + 1 + 28 + 3); e2e on 0.25 (shim) and 0.24 (fallback), SQLite and Postgres | covered, MySQL not run |
| canonical `table` / `schema`; non-canonical args rejected with a hint | `bus::events_carry_canonical_schema_and_table`, `capture::non_canonical_tables_are_refused_with_a_hint`, `observe::bad_args_are_rejected_at_creation`, `observe::new_consumer_…` (writes `Orders`, receives `orders`) | covered |
| `INVALID_ARGS` for a missing `db` and for `tabel` | `observe::bad_args_are_rejected_at_creation` (engine side) | covered |
| routing: `orders` vs `payments`, `ops`, table-less observer | `capture::routing_matches_db_table_and_ops`, `observe::new_consumer_…`, `observe::old_consumer_…` (`ops: [insert]`) | covered |
| legacy routing for `"Orders"` / `"public.orders"` | `observe::old_consumer_…` (`public.Orders`), e2e "table filter matches case-insensitively" | covered |
| a filter miss on `emit` sends nothing | `observe::unobserved_writes_put_zero_frames_on_the_wire` (filter-miss step) | covered |
| a native-capture rejection reaches `ready()` | `observe::native_path_…` (table-less), `capture::admission_checks_the_live_config` (install failure), e2e native cases | covered |
| install once per table; removal decrements | e2e "fans out … survives trigger reinstall" (behavior) | partly: the refcount has no direct test |
| config reload late-rejects observers on a removed database | `observe::config_reload_late_rejects_…` | covered |
| `caller.parent` on the statements path, absent on the native path | `observe::new_consumer_…`, `observe::native_path_events_have_no_caller_parent` | covered |
| commit-order reporting; rollback emits nothing | `observe::commit_order_and_rollback` (`max_in_flight: 1`) | covered |
| benchmark, 5k statements/s, Postgres | | not run |

Also covered by the prototype:

- **Zero wire.** A WebSocket proxy between the worker and the engine decodes
  every client frame. 1000 unobserved writes: 0 frames. 100 observed writes
  whose filter misses: 0 frames. 100 matching writes: 2 to 4
  `invocationsobserved` frames. 1000 writes after `dispose()`: 0 frames.
- **No injection.** `trigger('database::on-row-changed', forged)` from another
  worker fails with `FORBIDDEN` and no observer hears it (the worker's own
  refusal on the tree the prototype ran against; on the fixed 0.25 engine the
  engine refuses it before it reaches the worker, and the test passes
  unchanged).
- **Shim metadata, owner restart, config reload on both engines** (§5.1, §4.9).

Before merging the real migration, add: a direct refcount test (two observers
on one native table, remove one, the other still hears), the MySQL e2e run (CI
has the docker stack), and the benchmark.

## 7. Rollout order

1. **iii 0.25 release:** `iii-filter`, then `iii-helpers` and `iii-sdk`; the
   engine; the other SDKs.
2. **Workers repo SDK bump** (like #1375): widen `crates/console-ui`; bump
   `iii-sdk` / `iii-helpers` to 0.25 everywhere. The database worker needs no
   code change for this step (test A); add `#[allow(deprecated)]` or accept
   the 3 warnings.
3. **The database migration PR** (§4, plus the harness changes and the docs in
   §4.12). Release it as a minor version: it runs on 0.24 and 0.25 engines.
4. **Consumers, at their own pace:** the console feed (`useRowChanges.ts`), the
   skill and README examples, `onboarding/src/tours.mjs`, the
   `iii-directory` fixtures. All keep working through the shim meanwhile.
5. **Harness bindings later:** `harness/src/subscriptions`, `trigger_deliver`
   and their tests use `registerTrigger` with `__binding` metadata; they work
   through the shim and move to observers in phase 5/6, after durable
   observers exist.
6. **Removal** as in §5.4.

## 8. Risks and open questions

- **R1. Engine-features signal. Fixed in 0.25.** The tree the prototype ran
  against had no public way for a provider to know whether the engine speaks
  observe (`IIIClient::engine_observes` was `pub(crate)`), nor to hear when
  that changes on reconnect; hence the probe in §4.6. Its "observe hooks will
  not run" warning also listed only functions registered before
  `WorkerRegistered` arrived, and the database registers its function later,
  so against a 0.24 engine it never printed. The Rust SDK now has
  `IIIClient::engine_supports_observe() -> Option<bool>` and
  `on_engine_supports_observe(listener) -> ObserveSupportListener` (called
  when the value becomes known and on every change, off the connection's read
  loop; `unsubscribe()` stops it), and warns, once per function, for an
  observe function registered after the engine said it lacks observe. The
  migration uses them (§4.6, §5.2).
- **R2. Invocations of owner-delivered functions. Fixed in 0.25.** With a
  routing handler, any worker allowed to invoke `database::on-row-changed`
  could inject events, and the engine's report stamping made them look like
  the database's own (repro on `ba990e170` against the earlier tree: observe
  the function, then
  `trigger('database::on-row-changed', { db, table: 'orders', op: 'delete', … })`
  from another worker; the observer receives it with
  `caller.worker_name: "database"`). The engine now refuses an invocation of
  an owner-delivered function from any connection but the one hosting it
  (`FORBIDDEN`, `ObserverRegistry::may_invoke`, checked before the routed
  hook), which also closes it for `cron.md`'s routing handler. The database
  keeps routing in-process and refusing calls anyway (§4.1).
- **R3. Shim bindings in the trigger introspection functions. Fixed in
  0.25.** They were invisible to `engine::registered-triggers::list` /
  `::info` and `engine::triggers::list`, and 5 database e2e cases broke on
  that readiness check. The engine now lists each shim binding from the
  moment it is stored, `status: active` while the stand-in is registered, as
  an old trigger was listed as soon as it was stored, with `observer_state`
  and `served_by` added (§5.1); `engine::triggers::list` keeps the trigger
  type. Re-run on the fixed engine, the harness's original readiness check
  passes (70/70 on SQLite).
- **R4. Canonical args cannot name a schema or a case-sensitive table.** A new
  observer on a native Postgres table outside the search path, or named
  `"Orders"`, cannot be expressed: its install runs on the lowercase bare name
  and fails, and the observer's `ready()` says why (`CAPTURE_INSTALL_FAILED`
  with the case-sensitivity hint). `database.md` §2.2 records this as the
  migration's behavior. Follow-up: an optional `schema` arg, and resolving the
  bare name against `pg_class` (refusing an ambiguous match).
- **R5. One legacy shape for every legacy observer.** `legacy_view()` restores
  `public.orders` but cannot restore the writer's original spelling on the
  statements path (`INSERT INTO ORDERS` used to report `ORDERS`, now
  `orders`). Consumers that compared the raw string are affected; the harness
  is not.
- **R6. Engine version changes under a running worker** (§5.2): restart the
  worker; fixed by R1.
- **R7. console-ui still registers triggers**, so every worker linking it logs
  the `register_trigger` deprecation on each start until it moves to an
  observable function (`console::on-asset`, README §9).
- Open: whether the function's local invocation counts (lost by §4.1) are
  wanted in the engine's metrics; with R2 fixed, the local call is safe again
  on a 0.25 engine.

## 9. Checklist

- [ ] iii 0.25 published (`iii-filter`, `iii-helpers`, `iii-sdk`, engine)
- [ ] workers: `crates/console-ui` range widened, SDK bumped
- [ ] `sql.rs`: `canonical_table`, `split_table`
- [ ] `bus.rs`: `RowChangedArgs`, `schema`, `Deserialize`, `legacy_view`, buffer kept, subscribers removed, `emit` / `fan_out` route in-process
- [ ] `capture.rs`: function (refusing handler), hooks, index, `admit` keyed by installed spelling, `revalidate`
- [ ] `legacy.rs`: probe, fallback trigger type over the same index
- [ ] `handler.rs` deleted; native producers add `schema: None`
- [ ] `configuration.rs`: observe `configuration::on-updated` (trigger on 0.24); `apply_config` revalidates
- [ ] `main.rs`: capture before bus, register, probe, fallback
- [ ] tests: unit moved; `tests/observe.rs`; harness catalog list (the shim-aware readiness change in the branch is not needed on the fixed engine)
- [ ] direct refcount test, MySQL e2e, benchmark
- [ ] README, skill, descriptions; console feed later
- [ ] e2e green on a 0.24 and a 0.25 engine, SQLite, Postgres, MySQL
- [ ] `legacy.rs`: probe replaced by `engine_supports_observe()` / `on_engine_supports_observe` (R1, §4.6)
- [x] R3: shim bindings listed by the engine from creation; the harness's original readiness check passes
