# Worked example: `database::on-row-changed`

Source: `workers/database` at `079611a1f` (workers `origin/main`; the
`database/` tree is unchanged since `a1b51cac9`). Delivery: **owner**
(`obs.emit`): the database routes each event to the observers whose args
name its db, table and op. The migration plan, checked against the 0.25 engine
and SDKs with a prototype, is [`transition-database.md`](transition-database.md).

## 1. Today

- The worker registers the trigger type `database::row-changed`
  (`src/main.rs:690-707`). Its config is `{ db, table?, ops? }` with
  `deny_unknown_fields`.
- `RowChangedHandler::register_trigger` (`src/triggers/handler.rs`) validates the
  config:
  - unknown `db` → error;
  - a `capture: native` database requires `table`.

  It installs native capture (Postgres triggers + NOTIFY behind an advisory
  lock, a SQLite changelog plus triggers, MySQL binlog prerequisite checks), then
  files the binding in `RowChangeBus`.
- `RowChangeBus` (`src/triggers/bus.rs`) keeps its own subscriber list.
  `fan_out` matches each event against every subscriber:
  - `db` exact;
  - `table` case-insensitive and schema-insensitive through `sql::same_table`;
  - `ops`.

  It then sends one `TriggerAction::Void` call per matching subscriber, carrying
  the stored metadata. The metadata is load-bearing: it is the harness's
  `__binding` pointer.
- Events are emitted after commit.
  - On the statements path, from inside the mutating handlers
    (`src/handlers/mod.rs:116-158`: `emit` / `stage` / `commit` / `rollback`).
  - On the native path, from the listener tasks (`native.rs`, `sqlite_watch.rs`,
    `mysql_binlog.rs` → `emit_event`).
- **Granularity.** One event per committed write statement: `affected_rows`,
  plus `returning` with up to 100 keys and a `truncated` flag.
- **Delivery.** At-most-once ("a doorbell, not a ledger").

Today's design already sends **zero frames** for an event nobody subscribed to,
because the provider filters locally. The new design keeps that property. What
changes is that binding storage, filters, policy, ownership and lifecycle move
into the engine and the SDK. The database keeps only what is genuinely its own,
behind `onObserverAdded`: native capture, and routing by its own args (db, table,
ops), which only it understands.

## 2. Target

### 2.1 The function

```rust
pub const ROW_CHANGED_FN: &str = "database::on-row-changed";

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RowChangedArgs {
    pub db: String,
    pub table: Option<String>,           // canonical: bare name, unquoted, lowercased (§2.2)
    pub ops: Option<Vec<Op>>,            // insert | update | delete | other
}

let capture = Arc::new(NativeCapture::new(iii.clone(), config.clone(), pools.clone())); // §2.4

let row_changed = iii.register_function(
    ROW_CHANGED_FN,
    // An event source, not a callable: the bus routes each event in-process (§2.3)
    RegisterFunction::new(|_event: RowChangedEvent| Err::<(), _>(forbidden("observe it instead")))
        .description("One event per committed write statement. Observe it with args \
                      { db, table?, ops? } and filter on input.returning / caller.")
        // request_format comes from the handler's argument type, so RowChangedEvent derives
        // Deserialize + JsonSchema: the engine checks filter paths against it
        .observe(Observe::owner()                                         // the database picks who gets each event
            .args_format::<RowChangedArgs>()                               // was the trigger config
            .legacy_trigger_type("database::row-changed")
            .handler(capture.clone())),
);
capture.attach(row_changed.clone());   // the hooks hold the capture, so it exists before the FunctionRef
```

The function is what observers name, not a router anyone calls. The 0.25
engine refuses a routed call of an owner-delivered function from any
connection but its owner (`FORBIDDEN`), and the function's own handler refuses
every invocation as well, which also covers an engine older than 0.25. A
handler that routed would otherwise let any worker allowed to invoke it forge
events, and their reports would carry the database's identity. `RowChangeBus`
routes each event in-process instead (§2.3): the target receives the
`Observed` envelope (`capture: ['input']`, or the event itself with
`deliver: 'input'`), and an event costs one index lookup.

The trigger config becomes the observer's `args`. `args_format`
(`deny_unknown_fields`) rejects argument typos and a missing `db` at creation
(`tabel` → `INVALID_ARGS`), which is what `deny_unknown_fields` does for the
trigger config today, and `request_format` does the same for filter paths
(`input.tabel` → `INVALID_FILTER_PATH`).

### 2.2 Event shape: canonical values

The routing index (§2.4) matches `args.table` exactly, and so do filters
([`filters.md`](filters.md) §3). Today's tolerant table matching
(`sql::same_table`: last dotted segment, unquoted, lowercased) therefore moves
into the **event**:

```jsonc
{
  "db": "primary",
  "schema": "public",          // new; null when unknown
  "table": "orders",           // canonical: bare name, unquoted, lowercased (same rule as same_table)
  "op": "insert",              // insert | update | delete | other
  "affected_rows": 3,
  "returning": [ { "id": 41 }, { "id": 42 }, { "id": 43 } ],
  "truncated": false,
  "at": 1760350000123
}
```

- When the table cannot be classified (a CTE-wrapped write), `table` stays
  `null`, so it is routed only to observers whose args name no table:
  "silence beats a wrong match", as today.
- `onObserverAdded` rejects a non-canonical `args.table` (`"Orders"`,
  `"public.orders"`) with a hint naming the canonical value, so the mismatch
  fails at creation instead of matching nothing.
- `RowChangedEvent` derives `Deserialize` as well as `JsonSchema`: it is the
  function's input type, so its schema is the `request_format` that filter
  paths are checked against (`input.tabel` → `INVALID_FILTER_PATH`).
- **What canonical args cannot say.** `args.table` is a bare lowercase name,
  so a new observer cannot name a Postgres schema or a quoted mixed-case table
  (`"Orders"`). The migration does not guess:
  - on the statements path nothing is installed, and the canonical name
    matches every spelling of the table, as `same_table` does today; an
    observer that cares about the schema filters on `input.schema`;
  - on a native database the capture installs on `args.table` as given. That
    works for a lowercase table on the connection's search path; any other
    table fails at install (`CAPTURE_INSTALL_FAILED`, with today's "quoted
    identifiers are case-sensitive" hint), so the observer's `ready()` says
    why instead of listening to nothing;
  - a legacy binding keeps installing on the spelling it was written with
    (§2.5), so its behavior does not change.

  Native tables outside the search path need a later, optional `schema` arg,
  resolved against `pg_class` (an ambiguous match refused).

### 2.3 Emitting

`RowChangeBus` keeps the transaction buffer (`stage` / `commit` / `rollback`)
and loses `Subscriber`, `SubscriberFilter` and `targets_for`. The per-event
routing moves into `NativeCapture`, over an index the hooks maintain, and the
bus calls it in-process from the invocation that made the write.

```rust
pub struct RowChangeBus {
    iii: Arc<IIIClient>,
    capture: Arc<NativeCapture>,              // holds the FunctionRef and the index
    pending: Mutex<HashMap<String, Vec<Pending>>>,
}

async fn fan_out(&self, event: RowChangedEvent) {
    if !self.capture.is_listened() {           // row_changed.is_observed(): no serialization, no call, no frames
        return;
    }
    // In-process, on the task of the invocation that made the write: the SDK checks the routed
    // observers' filters and reports only matches. Awaited, so a transaction's statements are
    // reported in commit order.
    self.capture.route(&event).await;
}

impl NativeCapture {
    /// The args-based match RowChangeBus used to do per subscriber.
    async fn route(&self, event: &RowChangedEvent) {
        for route in self.index.matching(&event.db, event.table.as_deref(), event.op) {
            // the observer's filters run here; a miss sends nothing. Ok(false) on a miss or a removed
            // observer, so the loop needs no guard; an error is logged and the loop goes on
            let sent = if route.legacy { route.obs.emit(&event.legacy_view()) }   // §2.5
                       else { route.obs.emit(event) };
            if let Err(e) = sent { tracing::warn!(error = %e, "row-changed emit failed"); }
        }
    }
}
```

`route` is async because, against an engine older than 0.25, the same index
also holds the bindings of the fallback trigger type, which are called with
`void` as today ([`transition-database.md`](transition-database.md) §5.2).

- **Staging stays unconditional.** Skipping `stage()` while nothing is observed
  would miss an observer that becomes active between a statement and its
  commit. The commit *is* the invocation, so that observer is owed the
  event.
- **Caller context comes for free.** On the statements path `route` runs
  inside the `database::execute` (or `database::commitTransaction`) handler,
  on its task, and `emit` takes that invocation as the event's caller:

  ```jsonc
  "caller": { "worker_name": "database", "function_id": "database::execute", "invocation_id": "…",
              "parent": { "worker_name": "billing", "function_id": "billing::close-invoice", "invocation_id": "…" } }
  ```

  `caller.parent` answers "who changed this row".
- **The native path has no invocation context.** The listener tasks run
  outside any handler, so the event has only
  `caller: { worker_name: "database" }`. `caller.parent.*` is absent there, so
  `{"caller.parent.worker_name": {"$ne": "x"}}` still matches: we don't know,
  so we don't exclude.
- **Loop protection is automatic on the statements path.** A target that
  writes back runs at `observe_depth` 1, its `database::execute` call
  carries it, and the emit inside that call reports it. The engine stops the chain
  at the configured depth. A write from the native path, or by another client,
  has no depth: the same blind spot as today. A `caller.parent` filter is the
  explicit tool there.
- **No local dispatch involved.** Routing does not go through `trigger`, so it
  is the same with `observers.local_dispatch: false` or under RBAC middleware,
  where a local call of the function would turn into a round trip through the
  engine for every observed write.

### 2.4 Native capture and the routing index, driven by `onObserverAdded`

`RowChangedHandler` (the `TriggerHandler`) becomes `NativeCapture`, an
`ObserveHandler` on the function:

```rust
#[async_trait]
impl ObserveHandler for NativeCapture {
    async fn on_observer_added(&self, obs: &Observer) -> Result<(), ObserveRejection> {
        // `install` is the table as the DDL will name it
        let (args, install, legacy) = match obs.legacy() {
            Some(legacy) => {                                           // phase 5 shim, §2.5
                let (args, install) = self.legacy_args(&legacy.config)?;   // canonical args, the binding's spelling
                (args, install, true)
            }
            None => {
                let args = obs.args::<RowChangedArgs>()?;               // db required: checked by the engine
                if let Some(t) = &args.table { self.check_canonical(t)?; }   // "Orders" → hint "orders"
                let install = args.table.clone();
                (args, install, false)
            }
        };
        let cfg = self.config.read().await;
        let db_cfg = cfg.databases.get(&args.db)
            .ok_or_else(|| ObserveRejection::new("UNKNOWN_DB", format!("unknown db `{}`", args.db)))?;
        let mut installed = None;
        if db_cfg.capture == CaptureMode::Native {
            let Some(t) = &install else {
                return Err(ObserveRejection::new("NATIVE_CAPTURE_REQUIRES_TABLE",
                    format!("db `{}` uses capture: native; pass args.table", args.db)));
            };
            // the existing install path (advisory lock + DDL / changelog / binlog checks), once per
            // (db, table as installed); see §2.2 for what the name can be
            installed = Some(self.retain(&args.db, t).await?);
        }
        self.index.insert(obs.id(), &args, obs.clone(), legacy, installed);  // (db, table) → observers, with their ops
        Ok(())
    }

    async fn on_observer_removed(&self, obs: &Observer) {
        self.index.remove(obs.id());                               // and release its hold on the install
    }
}
```

`index.matching(db, table, op)` returns the observers on `(db, table)` and
those on `db` with no table, keeping those whose `ops` are absent or include
`op`. It is the args half of what `RowChangeBus` matched per subscriber; the
filters half now belongs to the observers.

- **Rejections.** A rejection moves that observer to `error`, and its
  observer's `ready()` rejects with the same message the trigger registration
  gives today.
- **One installer per table.** Capture is refcounted per `(db, table as
  installed)` across all observers ("first activates, last releases"), not
  once per binding. The key is the spelling the DDL used, not the canonical
  name: on Postgres `"ORDERS"` and `"orders"` are different relations, and a
  legacy binding spelled in the wrong case must fail its own install, as it
  does today, instead of riding on another binding's.
- **Indexed before the ack.** The observer is in the index before
  `on_observer_added` returns, so the first commit after the engine marks it `active`
  reaches it.
- **Teardown.** Installed capture stays installed when the refcount reaches
  zero. That is today's "ponytail" choice in `unregister_trigger`, unchanged.
- **Hot config reload** (`apply_config`) re-checks `row_changed.observers()`
  against the new config and sends a late rejection (`obs.reject(code, msg)`)
  for each observer that is no longer valid, e.g. one on a database that
  was removed. Legacy observers are not rejected: they stay filed and match
  again if their database comes back, as bindings do today. A rejection would
  remove them for good, and their registrant would never hear why.

### 2.5 Legacy callers

The function declares `legacy_trigger_type: "database::row-changed"`. An old
worker's `registerTrigger({ type: 'database::row-changed', config: { db, table, ops } })`
becomes an observer with `obs.legacy.config` set
([`protocol.md`](protocol.md) §6). `legacy_args`:

1. validates the config exactly as `RowChangedHandler` does today (unknown
   `db`, native without `table`; `on_observer_added` installs capture as for any
   observer);
2. returns `RowChangedArgs` with the **canonical** table, so the legacy
   observer joins the index exactly as a new observer with those args
   would, and the table **as the binding spelled it**, which native capture
   installs on (§2.4).

Because both the index and the event use the canonical name, a binding
written as `"Orders"` or `"public.orders"` keeps matching exactly what it
matches today. One index and one routing loop serve both kinds.

The shim delivers `deliver: "input"`, so a legacy target receives whatever the
owner emits to it. To receive the event as before, `route` emits
`event.legacy_view()` to a legacy observer: the pre-0.25 shape, without
`schema`, its `table` schema-qualified when a schema is known (`public.orders`
from Postgres native capture, as today). One difference remains: on the
statements path the table is lowercase (`INSERT INTO ORDERS` used to report
`ORDERS`). The trigger metadata is the sidecar, so harness bindings keep
working until they move to `observe`.

## 3. Observer examples

"Which worker changed `orders`?"

```ts
iii.registerFunction('audit::orders-changed', ({ input, caller }: Observed<RowChangedEvent>) =>
  audit.record({ op: input.op, keys: input.returning, by: caller?.parent?.worker_name ?? 'external' }))

iii.observe({
  function_id: 'database::on-row-changed',
  args: { db: 'primary', table: 'orders' },
  capture: ['input', 'caller'],
}, 'audit::orders-changed')
```

"Paid invoices only, statements capture with `RETURNING id, status`, a single
activation":

```ts
iii.observe({
  function_id: 'database::on-row-changed',
  args: { db: 'primary', table: 'invoices', ops: ['update'] },
  filters: { 'input.returning': { $any: { '@.status': { $eq: 'paid' } } } },
  capture: ['input'],
  deliver: 'input',
}, 'billing::on-invoice-paid', { max_fires: 1 })
```

"Everything except my own writes" (a projector that writes back):

```rust
iii.observe(
    Source::function("database::on-row-changed")
        .args(json!({ "db": "primary" }))
        .filters(json!({ "caller.parent.worker_name": { "$ne": "order-projector" } }))
        .capture([Capture::Input]),
    "orders::project",
);
```

## 4. Cost per event at 5k statements/s

| | Today (trigger) | New |
|---|---|---|
| No subscriber or observer | 0 frames. The provider's subscriber scan runs per event. | 0 frames. One atomic load; the event isn't even serialized. |
| 1 matching subscriber or observer | 1 frame database → engine (`void`), plus 1 frame engine → target | 1 emit → 1 report item (batched, ≤ 1 frame per 5 ms), plus 1 activation frame engine → target |
| 10 matching | 10 frames database → engine, plus 10 frames engine → targets | 10 emits coalesced into 1 report item, plus 10 activation frames |
| 10 observers, none matching | 0 frames | 0 frames. The index skips observers on other tables; the routed ones' compiled filters are evaluated in the SDK. |
| Predicate the trigger config cannot express (`returning` status, `caller`) | every event of the table is delivered and the target discards most | filtered in the SDK; only matches cross the wire |
| Binding storage, filter, lifecycle, ownership | in the database worker | in the engine and SDK; the worker keeps native capture and the args index |

## 5. Acceptance

- The existing database test suites pass on both the legacy (`registerTrigger`
  through the shim) and the new (`observe`) path.
- New tests:
  - canonical `table` / `schema` in events, and a non-canonical `args.table` rejected with a hint;
  - `INVALID_ARGS` for a missing `db` and for unknown args (`tabel`);
  - routing: an observer on `orders` never receives `payments` events, `ops` restricts the operations, and an observer with no table receives every table of its db;
  - legacy routing: `"Orders"` and `"public.orders"` bindings receive `orders` events, in the pre-0.25 shape (`public.orders` on Postgres native capture, no `schema`), with their metadata;
  - a wrong-case legacy binding on Postgres native capture fails its install while a correct one is live;
  - a direct invocation of `database::on-row-changed` is refused and reaches no observer;
  - a filter miss on `emit` sends nothing;
  - a native-capture rejection reaches the observer's `ready()`;
  - `onObserverAdded` installs once per table and indexes the observer; `onObserverRemoved` removes it and decrements;
  - config reload late-rejects observers on a removed database;
  - `caller.parent` on the statements path and its absence on the native path;
  - commit-order reporting inside a transaction;
  - rollback emits nothing.
- Benchmark: 5k statements/s, with 0, 1 and 10 observers at a 1% match rate, on
  Postgres in statements mode. Report the database worker's CPU, engine CPU,
  bytes on the wire, and p99 time from commit to activation.
