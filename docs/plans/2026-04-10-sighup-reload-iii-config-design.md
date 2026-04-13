# SIGHUP Reload for Engine Config — Design

**Status:** Draft (brainstormed 2026-04-10, pending eng review)
**Branch:** `sighup-reload-iii-config`
**Scope:** `engine/` only

---

## Goal

Allow an operator to change `config.yaml` on a running iii engine and apply the changes without restarting the process. The reload is triggered by sending `SIGHUP` to the engine. A bad config **stops the engine process** with a clear error message showing exactly where the problem is, so the operator is forced to fix it before restarting — no silent running with stale config.

## Non-goals (explicit)

- File watching (`notify`/`inotify`). Out of scope for v1.
- A new HTTP/CLI endpoint for reload. SIGHUP only.
- Graceful drain of in-flight invocations on changed or removed workers.
- Hot reload with zero blip for workers whose config actually changed.
- Partial reload (only some workers).
- Per-worker reload hooks on the `Worker` trait.
- Windows support for SIGHUP. Feature-gated to Unix.

## Core decisions

1. **Trigger:** SIGHUP only. Explicit operator action, matches the nginx/haproxy/postgres tradition.
2. **Scope of reload:** Diff & patch. Only workers whose `WorkerEntry` changed get touched. Unchanged workers keep running untouched (the entire point of reload vs restart).
3. **Changed-worker mechanism:** Destroy + recreate via existing `Worker` trait lifecycle. No new trait methods. Every worker supports reload on day one.
4. **Failure policy: exit on bad config.** When the new config is invalid (parse error, validation failure, duplicate names, etc.), the engine **exits with a non-zero status code** and a detailed error message showing the config file path and the exact location/cause of the error. This prevents operators from unknowingly running with a stale config after a failed reload. The operator must fix the config and restart the engine.

---

## Architecture

### Current lifecycle (as of `engine/src/workers/config.rs`)

```
CLI startup
    │
    ▼
EngineConfig::config_file(path)
    │  parse YAML, expand env vars
    ▼
EngineBuilder::new().with_config(cfg).build()
    │
    │  for each WorkerEntry (workers + modules + injected mandatory):
    │    registry.create_worker(...)  → Box<dyn Worker>
    │    worker.initialize().await
    │    worker.register_functions(engine)
    │    self.modules.push(Arc::from(worker))
    │
    ▼
EngineBuilder::serve()
    │  single shared (shutdown_tx, shutdown_rx) watch channel
    │  for each module: w.start_background_tasks(shutdown_rx, shutdown_tx)
    │  shutdown_rx.changed().await  ← blocks until external shutdown
    ▼
self.destroy() → walk modules, call w.destroy()
```

Workers live in `Vec<Arc<dyn Worker>>` local to `EngineBuilder`. There is no
path to mutate the running set after `build()`, and no way to know which
`WorkerEntry` produced which running `Worker`.

### New lifecycle with reload

```
CLI startup
    │
    ▼
EngineConfig::config_file(path) ───────┐
    │                                   │  path retained for reload
    ▼                                   │
EngineBuilder::new().with_config(cfg)   │
    │                                   │
    │  NEW: engine.begin_worker_scope(name)
    │       worker.register_functions(engine)
    │       let regs = engine.end_worker_scope()
    │                                   │
    │  stores RunningWorker {           │
    │    entry: WorkerEntry,            │
    │    worker: Arc<dyn Worker>,       │
    │    shutdown_tx: watch::Sender,    │  per-worker channel
    │    registrations: WorkerRegistrations,
    │  }                                │
    ▼                                   │
EngineBuilder::serve() ─────────────────┘
    │
    │  tokio::select! {
    │    _ = global_shutdown.changed() => break,
    │    Some(_) = sighup_stream.recv() => reload_manager.reload().await,
    │  }
    │
    ▼
shutdown → destroy all remaining RunningWorkers
```

### Key new types

```
ReloadManager {
    config_path: Option<String>,      // None ⇒ --use-default-config, SIGHUP is noop
    engine: Arc<Engine>,
    registry: Arc<WorkerRegistry>,
    running: Mutex<Vec<RunningWorker>>, // serializes reloads
}

RunningWorker {
    entry: WorkerEntry,
    worker: Arc<dyn Worker>,
    shutdown_tx: watch::Sender<bool>,
    registrations: WorkerRegistrations,
}

WorkerRegistrations {
    function_ids: Vec<String>,
    trigger_ids: Vec<String>,
    // extended over time as more engine state gets tracked
}

ReloadDiff {
    added:     Vec<WorkerEntry>,
    removed:   Vec<String>,        // by name
    changed:   Vec<WorkerEntry>,
    unchanged: Vec<String>,
}
```

---

## The reload procedure (six phases)

**Failure policy:** any error in Phases 1–5 **exits the engine process** with a
non-zero status code and a detailed error message. The operator must fix the
config and restart. This guarantees the running config always matches the file
on disk — no silent stale-config drift.

```
SIGHUP ──▶ ReloadManager::reload() acquires mutex
             │
             ▼
  ┌──────────────────────────────────────┐
  │ Phase 1: PARSE                       │
  │  read file → expand env → serde_yaml │
  │  on error:                           │
  │    log "reload: parse failed: <e>"   │
  │    (include file path + line/col)    │
  │    EXIT process with non-zero code   │
  └──────────────────────────────────────┘
             │
             ▼
  ┌──────────────────────────────────────┐
  │ Phase 2: NORMALIZE                   │
  │  - inject missing mandatory workers  │
  │    (mirrors build() at config.rs:392)│
  │  - keyed HashMap by name             │
  │  - reject duplicate names            │
  │  on error:                           │
  │    log "reload: duplicate worker     │
  │    name '<name>' in new config"      │
  │    EXIT process with non-zero code   │
  └──────────────────────────────────────┘
             │
             ▼
  ┌──────────────────────────────────────┐
  │ Phase 3: DIFF                        │
  │  compare against `running` by name:  │
  │   removed: in old, not new           │
  │   added:   in new, not old           │
  │   changed: name ∈ both, entry !=     │
  │   unchanged: rest                    │
  │  reject mandatory removal:           │
  │    log "reload: refused to remove    │
  │    mandatory worker '<name>'"        │
  │    EXIT process with non-zero code   │
  │  log "reload: diff +A -R ~C =U"      │
  └──────────────────────────────────────┘
             │
             ▼
  ┌──────────────────────────────────────┐
  │ Phase 4: DRY-RUN (validation)        │
  │  for entry in added ∪ changed:       │
  │    w = registry.create_worker(...)   │
  │    w.initialize().await?             │
  │    // NO register_functions,         │
  │    // NO start_background_tasks      │
  │    staging.push(w)                   │
  │  on any error:                       │
  │    log "reload: validation failed    │
  │    for '<name>': <e>"                │
  │    for w in staging: w.destroy()     │
  │    EXIT process with non-zero code   │
  └──────────────────────────────────────┘
             │
             ▼  (point of no return)
  ┌──────────────────────────────────────┐
  │ Phase 5: COMMIT                      │
  │  1. for each CHANGED:                │
  │     promote staged replacement:      │
  │       start_background_tasks(...)    │
  │       begin_worker_scope              │
  │       new.register_functions(engine) │
  │       regs = end_worker_scope        │
  │     then tear down old:              │
  │       old.shutdown_tx.send(true)     │
  │       old.worker.destroy().await     │
  │       engine.remove_worker_registrations│
  │           (&old.registrations)       │
  │                                      │
  │  2. for each REMOVED:                │
  │     shutdown_tx.send(true)           │
  │     worker.destroy().await           │
  │     remove_worker_registrations      │
  │                                      │
  │  3. for each ADDED:                  │
  │     same as "promote" above          │
  │                                      │
  │  on any failure (promote or destroy):│
  │    log "reload: COMMIT FAILURE: <e>" │
  │    EXIT process with non-zero code   │
  └──────────────────────────────────────┘
             │
             ▼
  ┌──────────────────────────────────────┐
  │ Phase 6: UPDATE STATE                │
  │  swap `running` to post-commit set   │
  │  log "reload: success"               │
  └──────────────────────────────────────┘
```

---

## State cleanup contract

**Problem:** Today, `Worker::register_functions(engine)` writes into
`engine/src/function.rs` `FunctionsRegistry` via `register_function` (line 82).
There is no corresponding unregister, and `Worker::destroy` in
`engine/src/workers/traits.rs:71` has a no-op default. A worker removed from
config would leave stale function IDs in the registry forever.

**Fix:** Track per-worker registrations via a scope on `Engine`.

```
engine.begin_worker_scope("foo::Bar")
    │  installs a builder that intercepts future registrations
    ▼
worker.register_functions(engine.clone())
    │  worker code is unchanged — still calls
    │  engine.functions_registry.register_function(...)
    │  but the interceptor records the IDs
    ▼
let regs: WorkerRegistrations = engine.end_worker_scope()
    │
    ▼
stored in RunningWorker.registrations
```

On destroy, the reload manager calls
`engine.remove_worker_registrations(&regs)` which walks the stored IDs and
removes them from the relevant registries.

Implementation: thread-local `RefCell<Option<ScopeBuilder>>` on `Engine`, or
`Arc<Mutex<Option<ScopeBuilder>>>` if `register_functions` may touch state
across thread boundaries. Worker implementations remain unchanged — this is
entirely invisible to them.

`EngineBuilder::build()` also starts using scopes so startup and reload share
one code path. No dual implementations to drift.

---

## Signal handling

**Where:** `EngineBuilder::serve()` at `engine/src/workers/config.rs:432` is
where the current shutdown loop lives. We extend it:

```rust
#[cfg(unix)]
let mut sighup = tokio::signal::unix::signal(
    tokio::signal::unix::SignalKind::hangup(),
)?;

loop {
    tokio::select! {
        _ = shutdown_rx.changed() => break,
        #[cfg(unix)]
        Some(_) = sighup.recv() => {
            tracing::info!("reload: SIGHUP received, reloading from {}", path);
            reload_manager.reload().await;
        }
    }
}
```

On Windows, the `#[cfg(unix)]` block disappears and the loop only handles
shutdown. Startup logs `reload: SIGHUP not supported on this platform` so
operators know.

**Default-config mode:** When `cli.use_default_config` was passed, there is no
file path to reload from. The reload manager's `config_path` is `None`, and
SIGHUP logs `reload: ignored, running with --use-default-config` and returns.

---

## Config equality

Two `WorkerEntry` values are "the same" if `name`, `image`, and `config`
(`Option<serde_json::Value>`) are all equal. `Value::PartialEq` is structural,
so `{a: 1, b: 2}` and `{b: 2, a: 1}` compare equal — users reformatting YAML
do not trigger spurious restarts. Whitespace and comments are invisible after
parsing, so they also never trigger reload.

---

## Logging contract

Every reload cycle produces a predictable log sequence. Operators grep for
`reload:` to see exactly what happened.

**Success path:**

```
reload: SIGHUP received, reloading from iii-config.yaml
reload: diff +2 added, -1 removed, ~1 changed, =7 unchanged
reload: success
```

**Failure paths — the engine exits after logging:**

```
reload: FATAL: parse failed: iii-config.yaml:42:5: unknown field 'portt', expected 'port'
reload: FATAL: duplicate worker name 'foo::Bar' in new config
reload: FATAL: refused to remove mandatory worker 'iii-engine-functions'
reload: FATAL: validation failed for 'my::Queue': port 5432 already in use
reload: FATAL: failed to start background tasks for 'my::Queue': bind error
reload: FATAL: destroy failed for changed worker 'my::Queue': timeout
```

Error messages include:
- The config file path
- For YAML parse errors: line and column number from `serde_yaml`
- For validation errors: the worker name and the specific error
- For duplicate names: the duplicated name

All failure paths use `tracing::error!` with the `reload: FATAL:` prefix,
then trigger `global_shutdown_tx.send(true)` to unwind `serve()` and exit
with a non-zero process status. This guarantees the operator sees the error
immediately and cannot miss a failed reload.

Success paths use `tracing::info!`.

---

## Testing strategy

### Unit

- **Diff logic** — pure function, table-driven:
  identical configs → all unchanged; add; remove; config-only change;
  image-only change; duplicate names in new config; mandatory removal attempt;
  `serde_json::Value` field-order irrelevance.
- **Scope tracking** — `begin_worker_scope` → register fake function →
  `end_worker_scope` → assert returned scope contains the function ID →
  `remove_worker_registrations` → assert the registry no longer has it.

### Integration

- **Full reload cycle** — spin up a real engine with a tmpfile config, assert
  a function is callable, rewrite the tmpfile, send SIGHUP via
  `nix::sys::signal::kill(getpid(), Signal::SIGHUP)`, wait for `reload: success`,
  assert new function is callable and old is not.
- **Unchanged workers do not blip** — open a long-running call on an unchanged
  worker, send SIGHUP that only mutates a different worker, assert the original
  call completes normally.
- **Bad YAML rollback** — rewrite tmpfile with a broken file, SIGHUP, assert
  `reload: parse failed` is logged and the old state still handles calls.
- **Validation failure rollback** — swap in a worker whose `initialize()`
  always fails, SIGHUP, assert `reload: validation failed` and old state intact.
- **Removed-worker cleanup** — remove a worker from config, SIGHUP, assert
  its function IDs are gone from `FunctionsRegistry`.
- **Mandatory injection** — remove a mandatory worker from config, SIGHUP,
  assert it is auto-injected and no error fires.

---

## Known risks

- **Exit on bad config stops everything.** A typo in the config file after
  SIGHUP kills all workers and drops all in-flight requests. This is by
  design — the operator is forced to fix the error immediately rather than
  running with stale config. Process supervisors (systemd, k8s) should be
  configured to NOT auto-restart on this exit code, since restarting with
  the same bad config would loop.
- **HTTP server port rebind blip** — if an operator changes a port, the TCP
  socket cycles. Documented behavior. Not fixed.
- **In-flight invocations on changed/removed workers are dropped.** Destroy is
  not graceful-drain. A follow-up could add `Worker::drain()`.
- **Rapid SIGHUPs** — the reload mutex serializes them. Second reload reads
  the current file contents, so a burst converges to the latest state.
- **File modified during parse** — `std::fs::read_to_string` is a single
  syscall. Partial reads are not a concern; malformed content triggers a parse
  error and the engine exits.
- **Thread-local scope on Engine** — if a worker spawns a task during
  `register_functions` that ALSO calls `register_function` from a different
  thread, the thread-local misses it. Mitigation: audit workers to ensure
  `register_functions` is synchronous and registers only on the calling
  thread. If violated, switch the scope to `Arc<Mutex<Option<ScopeBuilder>>>`.

---

## Open questions / follow-ups

- Should there be a CLI/HTTP trigger for reload in addition to SIGHUP? **Out of
  scope for v1.**
- Should the engine expose a `/reload` introspection endpoint showing the last
  reload result? **Out of scope for v1.**
- Should `Worker::drain()` exist for graceful in-flight handling? **Out of
  scope for v1, flagged as future work.**
