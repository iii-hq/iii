# Protocol and engine internals

Everything here is **additive** until phase 6:

- new message variants, sent only to peers that declared the matching feature;
- new optional fields on existing variants.

Current engines warn and ignore unknown message types
(`engine/src/engine/mod.rs`, the `json decode error` arm of the read loop).
Node, Python, Go and the browser SDK ignore them silently. The Rust SDK cannot
decode them: its enum has no `#[serde(other)]`, so it logs one warning per
frame. That is why the engine **must not** push new variants to a peer that did
not opt in.

## 1. Feature negotiation

- **Engine → worker.** `WorkerRegistered` gains `features: ["observe.v1", "local-dispatch.v1"]`.
- **Worker → engine.** The `engine::workers::register` payload gains `features: [...]`.
  - Features are set **once per connection**: the first `engine::workers::register` that carries `features` fixes them, and later declarations are ignored (debug log). The engine keeps at most 32 entries of at most 64 bytes each.
  - Node and the browser SDK send it before the registration replay. Rust sends it after.
  - Either order works, because the engine already buffers a connection's registrations until `workers::register` resolves its namespace (`is_namespaced_registration`, `engine/src/engine/mod.rs:150`).
  - If the namespace grace timer drains first, the connection is treated as legacy until its `workers::register` arrives. At that point the engine sends `functionregistered` for the functions the connection already registered.
- **The SDK enables `observe()`** only when the engine advertised `observe.v1`. Otherwise `observe()` fails with `UNSUPPORTED`.
- **The SDK enables local dispatch** for a function only after `functionregistered` arrived for it.
- **The engine pushes the new variants, and adds `caller`, `observe_depth` and `activation` to an `InvokeFunction`, only to connections that declared `observe.v1`.** It decodes every worker → engine message from anyone. The replies to a connection's own `observe` (`observeresult`, `observerstatechanged`) go to it regardless.
- **A function with `observe.delivery: owner` or with callbacks** needs `observe.v1` on both sides. Against an old engine the SDK registers it as a plain function and logs that its observe hooks are inactive.

| | old SDK | new SDK |
|---|---|---|
| **old engine** | n/a | `observe()` → `UNSUPPORTED`. No `functionregistered` ever arrives, so no local dispatch and no hooks; `trigger()` behaves as today. |
| **new engine** | No pushes. Every call is routed and evaluated by the engine. The SDK's functions **are** observable; it just cannot observe, and its trigger types keep working through the old path. | full |

## 2. Messages

All of them follow the existing `#[serde(tag = "type", rename_all = "lowercase")]` scheme.

### 2.1 Old → new

| Today | New | Direction |
|---|---|---|
| `registertriggertype {id, description, trigger_request_format, call_request_format}` | `registerfunction {…, request_format (was call_request_format), response_format, observe: {delivery, args_format (was trigger_request_format; owner only), legacy_trigger_type?, allow?}}`. `request_format` and `response_format` are omitted when absent (they used to be sent as `null`). | worker → engine |
| *(none: `registerfunction` has no success ack, only `registrationrejected` on conflict)* | `functionregistered {id, namespace, revision, observers}` | engine → owner |
| `registertrigger {id, trigger_type, function_id, config, metadata}` | `observe {id, source: {function_id, namespace, args, filters, capture, on, deliver}, target: {function_id, namespace}, metadata, max_fires, expires_in_ms, max_in_flight}` | observing worker → engine |
| `registertrigger` (forwarded to the provider) | `observeradded {function_id, revision, observer}` | engine → owner |
| `triggerregistrationresult` (provider's answer) | `observerack {function_id, id, revision, error?}` | owner → engine |
| `triggerregistrationresult` (to the registrant) | `observeresult {id, state, error?, warnings?}` + `observerstatechanged` | engine → observing worker |
| `unregistertrigger` | `unobserve {id}` | observing worker → engine |
| `unregistertrigger` (forwarded to the provider) | `observerremoved {function_id, revision, id}` | engine → owner |
| provider fires: one `invokefunction(target, void)` per binding | auto (matched local calls) and owner `obs.emit`: `invocationsobserved {items}` (batched), then engine activations; owner `obs.call`: `invokefunction {function_id: target, activation}` | owner → engine |
| `unregistertriggertype` (every SDK sends it; the engine had no variant until 0.25) | decoded and honored in 0.25 (the sending connection's trigger type is removed; from any other connection it is ignored with a debug log); removed in phase 6 | worker → engine |
| — | `localinvocationstats` | owner → engine |
| — | `InvokeFunction.caller`, `.observe_depth`, `.activation` | both |

The four trigger messages and `registertriggertype` are removed in phase 6.

### 2.2 Worker → engine

```jsonc
// RegisterFunction gains an optional `observe` descriptor. Callbacks are SDK-local and never sent.
{ "type": "registerfunction", "id": "http::on-request",
  "description": "One invocation per HTTP request. Observe it to register a route.",
  "request_format": { … HttpRequest … }, "response_format": { … HttpResponse … },
  "observe": {
    "delivery": "owner",                          // auto (default) | owner
    // owner only. args_format on an auto function → registrationrejected { code: "INVALID_OBSERVE_DESCRIPTOR" }
    "args_format": { "type": "object", "additionalProperties": false, "required": ["method", "path"],
                     "properties": { "method": { "type": "string" }, "path": { "type": "string" },
                                     "public_webhook": { "type": "boolean" },
                                     "middleware_function_ids": { "type": "array", "items": { "type": "string" } },
                                     "condition_function_id": { "type": "string" } } },
    "legacy_trigger_type": "http",                // phase 5 shim, optional
    "allow": true } }                             // false → NOT_OBSERVABLE
// An unknown `delivery` value → registrationrejected { code: "INVALID_OBSERVE_DESCRIPTOR" }.

// create, or idempotently replay, an observer. Owner = this connection.
// `source` is the left side of iii.observe, `target` the right side.
{ "type": "observe",
  "id": "5b0c…",                                  // client-generated UUID, stable across replays
  "source": {
    "function_id": "cron::on-tick",
    "namespace": "default",                       // optional; same resolution as InvokeFunction
    "args": { "expression": "0 0 9 * * Mon-Fri" },   // optional; validated against the function's args_format
    "filters": { "input.scheduled_time": { "$gte": "2026-11-01T00:00:00Z" } },   // optional; {} = every invocation
    "capture": ["input"],                         // auto and obs.emit; default ["input","output","caller"]; [] is valid (ids, status and timings only)
    "on": "success",                              // auto only: success | error | complete; default success
    "deliver": "envelope" },                      // auto and obs.emit: envelope | input | output; default envelope
                                                  // an unknown on / capture[] / deliver value → observeresult error UNSUPPORTED
  "target": { "function_id": "local::cron-handler", "namespace": "default" },   // a registered function
  "max_fires": 1, "expires_in_ms": 600000, "max_in_flight": 64,   // optional policy
  "metadata": { … } }                             // optional; the activation's metadata sidecar

{ "type": "unobserve", "id": "5b0c…" }

// owner acknowledges one added observer, after its onObserverAdded callback resolved.
// `error` rejects it (observer → error). The owner never changes the observer's filters.
// An owner may re-send an ack for an active observer at any time with an error
// (late rejection, e.g. a config reload removed a database). `revision` is the one the
// engine announced the observer with; an ack with an older revision is ignored as stale.
// `namespace` is optional (absent: the sending connection's namespace).
{ "type": "observerack", "function_id": "cron::on-tick", "namespace": "default",
  "id": "9e1…", "revision": 7,
  "error": { "code": "INVALID_CRON_EXPRESSION", "message": "invalid day-of-week field" } }   // when rejected

// matched local invocations of auto functions, and obs.emit events of owner functions,
// batched (≤ 256 items, ≤ 5 ms linger, 16 MiB frame cap)
{ "type": "invocationsobserved",
  "items": [ {
    "function_id": "database::on-row-changed", "namespace": "default",
    "invocation_id": "c3f…",
    "matched": ["5b0c…"],                         // auto: observer ids whose filters matched; owner: the emitted-to
                                                  // observers whose filters matched (same event coalesced)
    "status": "success",                          // success | error (always success for an emitted event)
    "input": { … }, "output": { … }, "error": { … },   // union of what the matched observers capture
    "caller": { "function_id": "database::execute", "invocation_id": "a1…",
                "parent": { "worker_id": "…", "worker_name": "billing", "function_id": "billing::close-invoice", "invocation_id": "77…" } },
    "metadata": { … },
    "started_at_ms": 1760350000123, "duration_ms": 0.004,
    "observe_depth": 0,
    "traceparent": "00-…", "baggage": "…" } ] }

// owner delivery (obs.call): an ordinary InvokeFunction to the observer's target, marked with the activation.
// The SDK sends no `namespace`: the engine routes to the observer's stored target (CONTRACT D13) and ignores one.
{ "type": "invokefunction", "invocation_id": "e7…", "function_id": "users::get",
  "data": { "method": "GET", "route": "/users/:id", "path": "/users/42", … },
  "activation": { "observer_id": "4f2…" } }

// periodic (every 10 s), only when non-zero
{ "type": "localinvocationstats", "window_ms": 10000,
  "functions": [ { "function_id": "database::on-row-changed", "count": 48211, "errors": 0, "duration_ms_sum": 190.2 } ],
  "dropped_reports": [ { "function_id": "database::on-row-changed", "reason": "BUFFER_FULL", "count": 12 } ] }
```

### 2.3 Engine → worker

```jsonc
// the RegisterFunction success ack, sent once per registration (and after a reconnect re-registration).
// Carries the full current set (`observers` is sent even when empty). Enables local dispatch for this
// function when `local_dispatch` is true. The owner acks EVERY listed observer again, with the revision
// of this message, including the ones it already held (a new registration starts a new owner relationship).
// `legacy` is omitted when absent.
{ "type": "functionregistered", "id": "cron::on-tick", "namespace": "default",
  "revision": 12, "local_dispatch": true,
  "observers": [ { "id": "9e1…", "args": { "expression": "0 0 9 * * Mon-Fri" },
                      "filters": { "$and": [ { "input.scheduled_time": { "$gte": "2026-11-01T00:00:00Z" } } ] },   // the observer's, normalized
                      "target": "local::cron-handler", "capture": ["input"], "on": "success" } ] }

// one observer attached to a function this connection hosts. Awaits observerack.
{ "type": "observeradded", "function_id": "cron::on-tick", "namespace": "default",
  "revision": 13,
  "observer": { "id": "a07…", "args": { "expression": "0 0 9 * * Mon-Fri" },
                   "filters": { … the observer's filters, normalized … },
                   "target": "billing::send-reminders", "capture": ["input"], "on": "success",
                   "legacy": { "trigger_type": "cron", "config": { … } } } }   // legacy trigger config, phase 5 shim only; the key is omitted otherwise

// one observer detached (unobserve, disposal, observer grace elapsed)
{ "type": "observerremoved", "function_id": "cron::on-tick", "namespace": "default",
  "revision": 14, "id": "a07…" }

// reply to observe (also sent on replay)
{ "type": "observeresult", "id": "5b0c…", "state": "pending",
  "error": { "code": "INVALID_FILTER_PATH", "message": "input.tabel is not in request_format" },  // when rejected
  "warnings": ["output.total: response_format is open; path not verified"] }

// lifecycle, sent to the observing worker
{ "type": "observerstatechanged", "id": "5b0c…", "state": "error",
  "reason": { "code": "HTTP_ROUTE_CONFLICT", "message": "Route 'GET /users/:uid' conflicts with …" }, "fires": 0 }
```

- The owner never receives an observer's owner connection, policy, metadata or
  `deliver`. It receives the id, the args (`{}` when none), the observer's
  filters (normalized), the target function id (not its namespace), capture and
  `on` (both always resolved), and `legacy` when the shim made it.
- `revision` is per `(namespace, function_id)` and increases by one on every
  add or remove. An SDK ignores an added/removed message whose revision is not
  greater than the one it holds on the same connection; `functionregistered` is
  always accepted and replaces the set (revisions restart after an engine
  restart).
- **Re-ack rule.** Until the owner acks an observer it stays `pending`, and
  after `observers.owner_ack_timeout_ms` it fails with `OWNER_TIMEOUT`. Every
  entry of a `functionregistered` is announced afresh and must be acked again,
  without calling `onObserverAdded` for an observer the SDK already holds (an
  observer whose `args` changed counts as removed, then added). An ack is
  accepted only with the `revision` of the announcement it answers.
  A late rejection uses the last revision the engine announced for that
  observer.

### 2.4 Additions to `InvokeFunction`

| Field | Direction | Meaning |
|---|---|---|
| `caller: { function_id?, invocation_id?, parent? }` | worker → engine | Sent by the SDK when the call is made inside a handler. `parent` is the caller of that handler, one level only. |
| `caller: { worker_id, worker_name, function_id?, invocation_id?, parent? }` | engine → worker | `worker_*` is stamped by the engine from the calling connection. It replaces the `_caller_worker_id` injection into `data` (`engine/src/engine/mod.rs:841-851`), which stays for one deprecation window. |
| `observe_depth: u32` | both | Depth of the causal chain in observe hops; omitted when 0. The SDK copies it from the current handler context into outgoing calls and reports. The engine sets `depth + 1` on activations. |
| `activation: { observer_id, activation_id? }` | both | Engine → worker: present on every activation. Worker → engine: marks an owner delivery (`obs.call`), validated in §3.5. |

`caller`, `observe_depth` and `activation` are added to an `InvokeFunction`
only when the receiving connection declared `observe.v1`. The engine reads
the three leniently: a malformed value is treated as absent. An `observe`
frame it cannot decode but whose `id` is a string is answered with
`observeresult` error `INVALID_ARGS`; an `invokefunction` it cannot decode but
whose `invocation_id` is a UUID is answered with an `invocationresult` error.
`caller.parent` is one level only (a `parent` inside `parent` is dropped).
The engine stamps `caller.worker_id` and `caller.worker_name` on every
routed invocation it forwards, and on every item of an `invocationsobserved`
report, overwriting what the SDK sent. The `_caller_worker_id` injection stays
for one release; an observed `input` never contains it.

## 3. Engine internals

### 3.1 New module `engine/src/observer/`

```rust
pub type FnKey = (String /*namespace*/, String /*function_id*/);

pub struct ObserverRegistry {
    by_id:       DashMap<ObsId, Arc<Observer>>,
    by_connection: DashMap<Uuid /*connection*/, HashSet<ObsId>>,
    by_function: DashMap<FnKey, Arc<FunctionEntry>>,
    /// Hot-path membership test for routed invocations: one lock-free load.
    observed: ArcSwap<HashSet<FnKey>>,
}

pub struct FunctionEntry {
    revision: AtomicU64,
    descriptor: ArcSwap<ObserveDescriptor>,       // delivery, args_format, legacy_trigger_type, allow
    active:  ArcSwap<Vec<Arc<Observer>>>,         // what the engine evaluates (auto) or accepts activations for (owner)
    all:     Mutex<Vec<Arc<Observer>>>,           // pending + active, for replay to a returning owner
    owner:   Mutex<Option<OwnerState>>,           // connection id, features, acked ids
}

pub struct Observer {
    id: ObsId, connection: Mutex<ConnectionRef>, function: FnKey, target: FnKey,
    args: Value,                                   // validated against args_format; sent to the owner
    filters: iii_filter::Filter,                     // the observer's; the owner never changes them
    filters_src: Value,
    capture: CaptureSet, on: On, deliver: Deliver, metadata: Option<Value>,
    policy: Policy,                                // max_fires, expires_at, max_in_flight, max_queue
    legacy: Option<LegacyTrigger>,                 // phase 5 shim
    state: AtomicU8, reason: Mutex<Option<ErrorBody>>,
    fires: AtomicU64, dropped: DropCounters,
    in_flight: Semaphore, queue: Mutex<VecDeque<Activation>>,
    added_in_revision: AtomicU64, created_at: Instant,
}
```

Files:

- `registry.rs`: the maps above, validation on `observe` (args against `args_format`, filters, RBAC, limits, self-target).
- `owner.rs`: `functionregistered`, `observeradded` / `observerremoved`, `observerack` (including late rejections), replay to a re-registered owner, park on owner disconnect. The pending / re-home / fallback-namespace logic in `engine/src/trigger.rs` (`pending_triggers`, `resolve_provider_key`, `rehome_fallback_bindings`) moves here, keyed by function instead of by trigger type.
- `dispatch.rs`: activations from reports (auto calls and owner emits), `obs.call` validation, policy, loop guard, metadata sidecar.
- `lifecycle.rs`: states, transitions, `observerstatechanged`.

### 3.2 Hooks into existing code

| Where | What |
|---|---|
| `invocation/mod.rs` `handle_invocation_inner` (the single choke point for every routed invocation) | For **auto** functions: before the handler runs, `registry.watch(&ns, &function_id)`. If observed, keep `body` only when a filter or capture needs `input`, and record the start time. After `result` is known (including `Deferred` → `receiver.await`), `registry.on_completion(watch, completion)` evaluates and dispatches without blocking the reply. **Requires threading `namespace` and the caller info (connection, `caller`, `observe_depth`) into the function**; today it receives only `worker_id` / `caller: Option<&WorkerConnection>`. Owner functions are skipped here. |
| `engine/mod.rs` `router_msg` | New arms for `Observe`, `Unobserve`, `ObserverAck`, `InvocationsObserved`, `LocalInvocationStats`. `InvokeFunction` reads `caller`, `observe_depth`, `activation`. |
| `RegisterFunction` accepted (after the `function_owners` claim) | Validate the descriptor: `args_format` with `delivery: auto` → `registrationrejected { INVALID_OBSERVE_DESCRIPTOR }`. Store it. `registry.on_function_registered(key, conn)`: send `functionregistered` (if the connection has `observe.v1`) with every pending and active observer, mark them `pending` awaiting acks; for a legacy connection, move them straight to `active`. Functions are unique per `(namespace, id)` (`FUNCTION_NAMESPACE_CONFLICT`), so there is exactly one owner. |
| `UnregisterFunction`, `cleanup_worker` | Observers **on** the owner's functions go back to `pending` (replayed when the function registers again). Observers **created by** the connection go `suspended(OBSERVER_DISCONNECTED)` and start the grace timer. Observers **targeting** its functions go `suspended(TARGET_UNAVAILABLE)`. |
| `Reattach` | Re-parent `by_connection[previous]` to the new connection and resume. A replayed `observe` with the same id and spec is a no-op returning the current state; a different spec → `OBSERVER_ID_CONFLICT`. The owner side needs nothing special: the replayed `RegisterFunction` gets a fresh `functionregistered`. A `Reattach` after the grace period, with the right token and within `disposed_retention_ms`, gets the disposed observers' tombstones instead: each replayed `observe` answers `disposed` with its reason, so `max_fires` and `expires_in_ms` are not reset. |
| worker connections / RBAC session | `local_dispatch` per function: global config ∧ the session has no `middleware_function_id` ∧ the connection may invoke the function itself. |
| `engine/src/trigger.rs` | Phase 5: shrinks to the legacy shim (§6). Phase 6: deleted. |
| engine-defined trigger types (`engine::functions-available`, `engine::workers-available`) | Become engine-hosted auto functions (`engine::on-functions-changed`, `engine::on-workers-changed`), evaluated by the engine itself (§3.7). The same holds for the trigger types of the engine workers still in use (`configuration`, `log`, `trace`). |
| `KNOWN_TRIGGER_TYPE_PROVIDERS` | Becomes a namespace → worker install hint shown on `pending` observers ("no worker hosts `http::on-request`; install `http`"). |

### 3.3 Lifecycle

```
 observe ─► pending ──── owner registered and acked (or owner is legacy), target available ────► active
              ▲  │                                                                                 │
              │  └─ owner rejected (observerack.error), invalid args/filters ───► error         │
              │                                                                                    │
              └────────────── owner disconnected or function unregistered ─────────────────────────┤
                                                                                                   ▼
             observing worker gone, target unavailable ────────────────────────────────────► suspended
             (resumes on Reattach within grace / when the target returns)
 unobserve │ max_fires reached │ expired │ observer grace elapsed │ error + observer disposes ──► disposed
```

- Only `active` observers are in `FunctionEntry.active`.
- Every transition sends `observerstatechanged` to the observing worker.
- Adding or removing an observer on a function bumps its revision and sends
  `observeradded` / `observerremoved` to the owner.
- `active` on an owner with local dispatch means the owner acked that
  observer. This is the completeness guarantee in README §6.
- A late rejection (`observerack` with `error` for an active observer)
  moves it to `error` and sends `observerremoved` back to the owner.

### 3.4 Reports: auto calls and owner emits

```
on_completion / on_report(item):
  for obs in matched (engine-evaluated, or item.matched ∩ active set for the reporter's function):
    if obs.state != active                     → drop(INACTIVE)
    if depth + 1 > max_observe_depth       → drop(LOOP_GUARD); warn once per obs per minute
    if expired                                 → dispose(EXPIRED); drop
    if max_fires and fires.fetch_add(1) >= max → drop(MAX_FIRES)
    envelope ← project(item, obs.capture, obs.deliver)
    if in_flight.try_acquire()                 → spawn invoke
    elif queue.len() < max_queue               → enqueue
    else                                       → drop(OVERFLOW)
    if fires == max_fires                      → dispose(MAX_FIRES) after dispatch
invoke:
  call(target_ns, target_fn, envelope,
       invocation_id = fresh,                     # no action: an ordinary call the target answers
       caller = { worker_name: "iii-engine", function_id: observed fn, invocation_id: observed invocation },
       activation = { observer_id, activation_id },
       observe_depth = depth + 1,
       traceparent = observed invocation's traceparent,
       metadata = obs.metadata)
  on reply or after activation_timeout_ms        → free the in_flight slot, start the next queued activation
```

- **Activations carry an `invocation_id`.** The target's SDK answers them like
  any call. The engine uses the reply to free the observer's `max_in_flight`
  slot and discards the result of auto and `obs.emit` activations (the observed
  side is fire-and-forget). An activation the target never answers frees its
  slot after `observers.activation_timeout_ms` and counts as a `TIMEOUT`
  failure.
- **Activation failures** are not drops: the event was on its way. They are
  counted by reason (`TIMEOUT`, `TARGET_UNAVAILABLE` for a gone target, a
  stopped invocation or `function_not_found`, `TARGET_ERROR` for the target's
  own error) in `iii.observer.activation.failures.total` and in
  `engine::observers::list`.
- **Cancelled calls.** An observed invocation dropped before its result is
  known (its caller gave up) counts a `CANCELLED` drop for each observer that
  was waiting for it, since nobody can know which of them would have matched.
- **Loop guard through workers without `observe.v1`.** A 0.24 SDK cannot carry
  `observe_depth` back, so a chain through it would restart at depth 0. For a
  connection without `observe.v1` the engine remembers the depth of each
  in-flight invocation it forwarded at depth > 0 and treats any call that
  connection makes meanwhile as being at the deepest of those depths. The guard
  is advisory there: an unrelated call made while a deep activation is being
  handled can be dropped (false positive).

- **Reports are validated first.** The reporting connection must be the current
  owner of `(namespace, function_id)` in `function_owners`. Otherwise the item
  is dropped and counted (`REPORTER_NOT_OWNER`). `matched` ids that are not
  active observers of that function are dropped (`INACTIVE`).
  Items are checked one by one: an item that does not decode, or whose
  `status` is neither `success` nor `error`, is dropped alone (`INVALID_REPORT`)
  and the rest of the batch is applied.
- **Both delivery modes report the same way.** For an `auto` function an item
  is a local invocation whose filters the SDK evaluated. For an `owner`
  function it is an `obs.emit`: the owner picked the observers from their
  args, and the SDK applied their filters. `on` is not checked for owner
  functions.
- **The engine does not re-evaluate filters on reports.** The owner is
  authoritative for its own function's data.
- **Activation authority (v1).** Activations run like today's trigger fires,
  with no session. Creation-time RBAC on the observed function and the target
  is the gate.

### 3.5 Owner delivery: validating `activation`

`obs.call` checks the observer's filters in the SDK first; a miss rejects with
`FILTERS_NOT_MATCHED` and sends nothing. An `InvokeFunction` from a worker that
carries `activation.observer_id`:

1. The observer exists → else `OBSERVER_NOT_FOUND`.
2. The sender is the current owner of the observed function → else `NOT_FUNCTION_OWNER`.
3. The observed function is `delivery: owner` → else `NOT_OWNER_DELIVERED`.
4. The observer is `active` → else `OBSERVER_NOT_ACTIVE`.
5. `function_id` equals the observer's target. The SDK fills it from the
   `target` it received; a mismatch → `TARGET_MISMATCH`. The engine routes to
   the observer's stored target `(namespace, function_id)` and ignores any
   `namespace` on the request; SDKs send none.
6. Policy, in order: depth (`LOOP_GUARD`), expiry and `max_fires` (both
   `OBSERVER_NOT_ACTIVE`; an expired observer is also disposed), `max_in_flight`
   (an owner delivery over the limit fails with `OVERFLOW` rather than queueing,
   because the owner is waiting for a result).
7. The engine adds `metadata = obs.metadata`, `activation_id`, and
   `observe_depth + 1`, and routes the call as a normal invocation. The
   result returns to the owner as an ordinary `InvocationResult`. A target
   that does not answer within `observers.activation_timeout_ms` fails the
   call with `TIMEOUT`; an unavailable target surfaces as `function_not_found`
   or `invocation_stopped`, and any other failure as the target's own error.

Errors in 1–6 come back as the `InvocationResult.error` of that invocation, so
the owner's `obs.call()` rejects with the code.

### 3.6 Builtins, config, metrics, RBAC

**Builtins**
- `engine::observers::list({ function_id?, namespace?, worker? })`: RBAC-filtered to observers whose function the caller may invoke. Returns id, function, namespace, args, filters, target, observing worker, delivery, state, reason, fires, drops by reason and activation failures by reason, policy and `legacy` (never metadata).
- `engine::observers::get({ id })`: an observer the caller may not see is `OBSERVER_NOT_FOUND`.
- `engine::observers::dispose({ id })`: the observing worker, or an unrestricted caller (in-process, or a worker manager connection without an `rbac` section).
- There is **no ownerless create function**. `engine::register_trigger`'s ownerless path, which skips the hook and is lost on restart, is not repeated.

**Config**

```yaml
observers:
  local_dispatch: true            # global kill switch for SDK in-process calls
  max_per_connection: 1024
  max_per_function: 4096
  max_observe_depth: 8
  observer_grace_ms: 30000
  disposed_retention_ms: 3600000  # a Reattach after the grace period still learns the disposal
  owner_ack_timeout_ms: 30000     # an add not acked in time → error(OWNER_TIMEOUT)
  activation_timeout_ms: 30000    # an activation or obs.call unanswered in time frees its slot (TIMEOUT)
  default_max_in_flight: 64
  max_queue_per_observer: 1024
```

Validation: the section is a top-level key of the engine config and every key
is optional (an absent section is all defaults). Unknown keys are refused. A
count or depth of 0 (`max_per_connection`, `max_per_function`,
`max_observe_depth`, `default_max_in_flight`, `max_queue_per_observer`) is
refused at load, because 0 would not mean "no limit". A timeout of 0 loads with
a warning. There is no hot reload: changes need a restart. `local_dispatch` is
computed per function at registration.

**Metrics** (per function, not per observer, to bound cardinality)
OTel names are dotted; the Prometheus exporter shows them as `iii_observer_*`.
- `iii.observer.activations.total{function_id, delivery}`
- `iii.observer.dropped.total{function_id, reason}`: engine reasons, the SDK reasons `BUFFER_FULL` / `TOO_LARGE`, and `OTHER` for any SDK reason the engine does not know (a peer cannot mint labels).
- `iii.observer.reports.total{function_id}`
- `iii.observer.activation.failures.total{function_id, reason}`: `TIMEOUT`, `TARGET_UNAVAILABLE`, `TARGET_ERROR`.
- `iii.observers{state}` (gauge)
- `iii.invocations.total{function_id, status, dispatch="local"}` (`status` is `ok` or `error`), fed by `localinvocationstats`. Without it, the invocation counts for locally dispatched functions would fall to zero.

**RBAC**
- `RbacConfig.on_observer_registration_function_id`: same contract as `on_trigger_registration_function_id` (which stays for the legacy path in 0.25). The hook may rewrite `function_id`, `target_function_id`, `args`, `filters` and `metadata`; a denial is `FORBIDDEN`.
- Creating an observer requires `is_function_allowed` for both the observed function and the target under the session, and the check is repeated when the target registers again on another connection.
- `observe.allow: false` makes a function unobservable (`NOT_OBSERVABLE`).

### 3.7 Engine-hosted event sources

The engine's own trigger types, and those of the engine workers still in use,
are auto functions in namespace `default`, observed like any other. Each event
is one successful invocation whose `input` is the event and whose output is
empty, with the engine as the caller. Nothing is built while nothing observes
the function. Use `deliver: input`, since they have no output. Their
`request_format` describes the event (closed at the top level), so an
observer's `input.*` filter paths are checked. Calling one directly is
refused with `FORBIDDEN`: its events are the engine's to emit. The old trigger
types keep firing.

| Function | Replaces trigger type | Event (`input`) |
|---|---|---|
| `engine::on-functions-changed` | `engine::functions-available` | `{ event: "functions_changed", functions: [{ function_id, namespace, worker_name, description?, metadata? }] }`; a burst is coalesced over about 100 ms |
| `engine::on-workers-changed` | `engine::workers-available` | `{ event: "worker_connected" \| "worker_disconnected" \| "worker_metadata_updated", worker_id }` |
| `configuration::on-updated` | `configuration` | `{ type: "configuration", event_type, id, name, description, schema, old_value, new_value, metadata? }` (filter on `input.id`, `input.event_type`) |
| `observability::on-log` | `log` | one log record in OTel fields (`LogCallRequest`: `severity_text`, `severity_number`, `body`, `service_name`, `trace_id`, `span_id`, `attributes`, `resource`, ...) |
| `observability::on-span` | `trace` | `{ trace_ids: [...] }` per window of about 300 ms |

**Depth and loops.** An event has the observe depth of what caused it, so the
loop guard (`max_observe_depth`, §3.4) holds for a target that causes the
next event itself:

- `on-functions-changed`: the deepest depth among the registrations the burst
  coalesces (an in-process registration made inside an activation, or one a
  worker sends while it runs one).
- `on-workers-changed`, `configuration::on-updated`: the depth of the call
  that caused it (`engine::workers::register`, `configuration::set`, ...).
- `on-log`, `on-span`: the depth recorded with each record and span when it
  is ingested: the engine invocation's depth for `engine::log::info`, the
  engine's own log events and its invocation spans (`iii.observe_depth`
  attribute); for a worker's OTLP records and spans, the deepest invocation
  its connection is running when they arrive. That last one is an
  approximation, for `observe.v1` workers too: telemetry carries no depth, an
  exporter that batches may deliver a record after the invocation that
  produced it has answered (it then counts as depth 0), and unrelated records
  sent while a deep invocation runs take its depth. `on-span` takes the
  deepest span of its window.

**Fresh trace per activation (`on-log`, `on-span`).** Unlike §3.4, where an
activation carries the observed invocation's `traceparent`, each event of
these two starts a trace of its own, shared by its activations. Records and
spans in such a trace are the targets' reaction to an event, not new events,
and are skipped, which stops a target that logs or calls back into what it
observes even where depth is lost (a batching exporter). The trade-off: a
target's own logs and spans do not show up as events of the source that
triggered it, and its activity is not nested under the record's trace (a span
link to it is not added). The engine remembers the last 4096 such traces for
ten minutes; past that, records in a forgotten trace count as events again and
the loop is stopped by depth alone.

## 4. Error codes

These strings are the wire contract (`engine/src/protocol.rs`, `observe_codes`).

| Code | When |
|---|---|
| `UNSUPPORTED` | The engine lacks `observe.v1` (raised by the SDK), or `observe` carries an unknown `on` / `deliver` / `capture[]` value (raised by the engine; the message names the field). |
| `INVALID_ARGS` | `args` fail the function's `args_format` (a missing required argument included), or a function without `args_format` (every `auto` function) receives non-empty `args`; an undecodable `observe`; a malformed legacy trigger id. |
| `INVALID_OBSERVE_DESCRIPTOR` | `registerfunction` declares `args_format` with `delivery: auto`, or an unknown `delivery` (`registrationrejected`; the SDK also throws at `registerFunction`). |
| `INVALID_FILTER` / `INVALID_FILTER_PATH` | Filter validation fails, including a literal where an operator object is expected ([`filters.md`](filters.md) §4). |
| `INVALID_NAMESPACE` | `source.namespace` or `target.namespace` is present and blank. Absent means `default`. |
| `NOT_OBSERVABLE` | The function registered `observe.allow: false`. |
| `FORBIDDEN` | RBAC on the observed function or the target, or the registration hook; also a direct call to an engine-hosted event source (§3.7), and a call to an owner-delivered function from anyone but its owner (§5). |
| `SELF_TARGET` | The target equals the observed function. |
| `LIMIT_EXCEEDED` | Per-connection or per-function caps. |
| `OBSERVER_ID_CONFLICT` | A replay with the same id but a different spec. |
| `OWNER_TIMEOUT` | The owner did not ack an added observer within `owner_ack_timeout_ms`, or its `onObserverAdded` hook ran past 30 s. |
| *(owner rejections)* | Carried verbatim from `observerack.error`; `OBSERVE_REJECTED` when the callback threw a plain error. Defined by the worked examples: `HTTP_ROUTE_CONFLICT` ([`http.md`](http.md)); `INVALID_CRON_EXPRESSION`, `CRON_CONDITION_UNSUPPORTED` ([`cron.md`](cron.md)); `UNKNOWN_DB`, `NATIVE_CAPTURE_REQUIRES_TABLE` ([`database.md`](database.md)). Missing required arguments are `INVALID_ARGS`, raised by the engine before the owner is asked. |
| *(owner delivery, §3.5)* | `OBSERVER_NOT_FOUND`, `NOT_FUNCTION_OWNER`, `NOT_OWNER_DELIVERED`, `OBSERVER_NOT_ACTIVE`, `TARGET_MISMATCH`, `OVERFLOW`, `LOOP_GUARD`, `TIMEOUT` (the target did not answer within `activation_timeout_ms`), `function_not_found` / `invocation_stopped` (target unavailable), `invocation_error`, or the target's own error code |
| *(owner delivery, SDK-side)* | `FILTERS_NOT_MATCHED` (`obs.call` on a payload the observer's filters reject; nothing sent), `NOT_OWNER_DELIVERED` (`obs.emit` / `obs.call` on an auto function), `OBSERVER_NOT_ACTIVE` (`obs.call` on a removed or rejected observer) |
| *(state reasons)* | `OWNER_UNAVAILABLE`, `TARGET_UNAVAILABLE`, `OBSERVER_DISCONNECTED`, `EXPIRED`, `MAX_FIRES`, `UNOBSERVED` |
| *(drop reasons)* | `INACTIVE`, `LOOP_GUARD`, `MAX_FIRES`, `OVERFLOW`, `REPORTER_NOT_OWNER`, `CANCELLED` (the observed call was cancelled before its result), `INVALID_REPORT` (a malformed report item), `BUFFER_FULL` / `TOO_LARGE` (SDK-side, via stats; an SDK reason the engine does not know is counted as `OTHER`) |
| *(activation failures)* | `TIMEOUT`, `TARGET_UNAVAILABLE`, `TARGET_ERROR`: counted, not drops (§3.4) |

## 5. Security notes

- **Caller identity.** `caller.worker_*` is engine-stamped and cannot be forged.
  `caller.function_id`, `invocation_id` and `parent` are asserted by the calling
  SDK. They are useful for routing and filtering, not as an authorization input.
- **Owners see args, filters and target ids.** Args and filters can contain
  sensitive literals, such as a customer id. Owners never see who observes, the
  observer's metadata or its policy. Whether a `private` observer
  (engine-only evaluation, with the owner reporting every invocation) is needed
  is open.
- **Owners can only report and deliver for their own functions.** Both are
  checked against `function_owners`. An owner can fabricate invocations of its
  own function, but it already controls everything that function returns.
- **Only the owner invokes an owner-delivered function.** Its events are what
  the owner reports, with `caller.worker_*` stamped from the owner's
  connection, so a call from anyone else would let a worker forge events that
  look like the owner's (a row change "from" the database worker) and that no
  `caller` filter could tell apart. The engine refuses an `InvokeFunction` of
  a function registered with `observe.delivery: owner` from any connection
  but the one that owns it, with `FORBIDDEN`; the owner may call itself (it
  does when it does not dispatch locally). Engine-internal calls (hooks,
  middleware, an old trigger bound to it) are refused the same way: the
  engine has no reason to originate such an event. Auto functions are not
  restricted. For the same reason an owner-delivered function cannot be an
  observer's target: `observe` refuses it with `FORBIDDEN`, and an observer
  whose target later registers as owner-delivered moves to `error`
  (`FORBIDDEN`).
- **Owner-delivered functions are entry points.** Observing `http::on-request`
  registers a route; calling it runs the router. RBAC must not expose it to
  untrusted sessions, and observing it may need a permission of its own
  (README §11.9).
- **Owners pick observers; observers own filters.** An owner chooses which
  of its function's observers receive an event, from their args, and never
  edits filters; its SDK applies the observer's filters before anything is
  sent. As with reports, the engine does not re-evaluate them: the owner is
  authoritative for its own function's data.
- **The rbac-proxy worker** (`tech-specs/2026-06-22-rbac-proxy-worker/`) must
  learn the new variants:
  - forward `functionregistered`, `observeradded`, `observerremoved`, `observerack`, `invocationsobserved` and `localinvocationstats`;
  - gate `observe` like `RegisterTrigger` today;
  - rewrite `engine::observers::list` like the other discovery functions.

## 6. Legacy shim (phase 5)

A function declares the trigger type it replaces:

```json
"observe": { "delivery": "owner", "legacy_trigger_type": "cron" }
```

When `RegisterTrigger { trigger_type: T, function_id: F, config: C, metadata: M }`
finds no live old-style provider for `T`, but a registered function declares
`legacy_trigger_type == T`, the engine:

1. **Finds the stand-in** the way `RegisterTrigger` finds a provider: in the
   trigger's named namespace only when it names one, otherwise in the
   registering connection's namespace and then in `default`. Only a function
   whose owner declared `observe.v1` stands in (the shim needs the owner's ack
   to answer the registrant); `legacy_trigger_type` on any other owner is
   ignored. With several candidates in one namespace the smallest function id
   wins.
2. **Creates an observer** on that function with
   - observing worker = the registering connection;
   - **observer id = the trigger id** (any non-empty string: like the triggers
     it replaces, it has no length bound, and these observers count against
     no `max_per_connection` / `max_per_function` limit), so
     `UnregisterTrigger` finds it and a re-registration with the same id
     replaces it, as it replaces a trigger;
   - `source: { function_id: <that function>, args: {}, filters: {}, deliver: "input" }`
     (so the target receives what it received from the trigger);
   - `target: F`, `metadata: M`;
   - `legacy: { trigger_type: T, config: C }`, forwarded in `observeradded`
     and `functionregistered`. The engine skips `args_format` validation for
     it: `C` is the old config, and the owner translates it.
3. **The owner's `onObserverAdded`** sees `obs.legacy`, validates `C` exactly as its
   `TriggerHandler` did, reads it as its args and routes the observer like
   any other: cron joins the expression's job, the database joins its index
   with the canonical table, HTTP inserts a route.
4. **The registrant gets one `TriggerRegistrationResult`** with the outcome: a
   success once the owner acked, or the owner's error if it rejected (SDKs
   ignore the success). The engine keeps the filters `{}` and activates.
5. **`UnregisterTrigger`** (and `engine::unregister_trigger`) disposes the
   observer, and its owner hears `observerremoved`.

Related rules:

- **Provider precedence.** If `T` has a live old-style provider, it wins for
  every new registration. Observers the shim already made stay where they are
  when such a provider registers later.
- **Parked bindings.** Bindings parked because nothing provided their type
  become observers when a stand-in registers (only those of a connected worker;
  an ownerless binding from `engine::register_trigger` stays parked for an
  old-style provider).
- **Owner leaving.** The observer waits and is replayed like any other
  observer when the owner registers again.
- **Introspection.** Shim bindings are listed by
  `engine::registered-triggers::list` / `::info` like old bindings, from the
  moment they are stored (also while the owner's ack is in flight), with
  `served_by` (the stand-in function) and `observer_state`; `status` is
  `active` while the stand-in is registered and `pending` while it is away. A
  type a stand-in serves stays in `engine::triggers::list` / `::info`.

Workers that still call `registerTrigger` keep working, and the provider
deletes its `TriggerHandler`, binding table and fan-out in the same release that
adds the function. In 0.25 the engine logs these registrations at `debug`, and
the SDKs warn once per process.

