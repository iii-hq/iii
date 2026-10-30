# observe v2 — implementation decisions

Binding decisions for implementing [`README.md`](README.md) across the engine,
the five SDKs, the CLI and `iii-hq/workers`. Where the spec and this file
differ, this file wins, and the spec is updated to match.

## Engine and wire

1. **Owner delivery is gone from the engine.** No `observe.delivery`, owner
   emits in `invocationsobserved`, or owner-call checks
   (`NOT_OWNER_DELIVERED`, `TARGET_MISMATCH`). A `registerfunction` whose
   `observe` object says `delivery: owner` is refused with
   `registrationrejected { code: UNSUPPORTED }`. The 0.25 `observe` object is
   otherwise read as decision 37 says, from any connection.
2. **Features.** The engine advertises `observe.v1`, `observe.v2`,
   `local-dispatch.v1`. v2 SDKs declare `observe.v1` and `observe.v2`. `v2`
   is a superset: owner sync, `caller`, `observe_depth` and `activation` work
   as for v1. A host that declared `observe.v2` delivers its own routed calls;
   the engine evaluates routed calls only for hosts without it and for
   engine-hosted functions.
3. **Routed completion.** `Invocation.sender` carries a completion struct
   (`result`, `observed: Vec<String>`) instead of a bare result. The
   `InvocationResult` arm fills it; the routed hook activates the listed ids
   after checking each belongs to the invoked function and is active, using
   the input it kept (the routed hook keeps the input exactly when the function
   is observed, as in 0.25).
4. **`output_is_input`** exists on `invocationsobserved` items only. An
   `invocationresult` always carries the result, which the engine already uses
   as `output`. SDK detection: Node, browser and Python compare by identity
   (`output === input`, `output is input`); Rust and Go compare the serialized
   bytes, and only when building a report item that carries both. An SDK sets
   it only when the engine advertised `observe.v2`; to an older engine the item
   carries `output`.
5. **Removed trigger frames** (`registertriggertype`, `unregistertriggertype`,
   `registertrigger`, `unregistertrigger`, `triggerregistrationresult`): the
   engine logs one `warn` per connection and message type naming the
   replacement (`iii.observe` / an observable function), and drops the frame.
   The generic undecodable-frame warning is not logged for these.
6. **Builtins.** Removed: `engine::register_trigger`,
   `engine::unregister_trigger`, `engine::triggers::list|info`,
   `engine::registered-triggers::list|info`. Every function row (in
   `engine::functions::list`, `engine::functions::info` and the
   `engine::on-functions-changed` event) carries `observer_count` always,
   `args_format` when set, `private: true` when private and
   `observable: false` when not observable; `registered_triggers` is removed.
   `engine::workers::info` carries `functions` and `observers` (ids of the
   observers the worker created); `trigger_types` and `registered_triggers`
   are removed.
   New engine-hosted source `engine::on-observers-changed`
   (`{ observer_id, function_id, target, state, reason? }`, one call per state
   change).
7. **Engine-hosted sources** are private functions whose handler returns the
   event, so `input` and `output` are both the event. Existing ones keep their
   ids (`engine::on-functions-changed`, `engine::on-workers-changed`,
   `configuration::on-updated`, `observability::on-log`,
   `observability::on-span`). iii-stream gets `stream::on-changed`,
   `stream::on-join`, `stream::on-leave` (no args; filter on
   `input.stream_name`, `input.group_id`).
8. **Engine-owned observers.** `ObserverRegistry` gains observers created by
   the engine itself (owner = engine, never suspended, not counted against
   connection limits), used by engine modules that consumed triggers
   (telemetry topics on `queue::on-message`, Console UI assets). In-process
   sources (configuration reload) use direct Rust listeners.
9. **Queue compat module** (`QueueWorker`, `durable:subscriber` in the
   engine) is deleted.
10. **RBAC.** `allow_trigger_type_registration`, `allowed_trigger_types`,
    `on_trigger_registration_function_id` and
    `on_trigger_type_registration_function_id` are removed. The two
    `on_trigger_*` config keys, when present in an `rbac` config, are ignored
    with one `warn` at load naming the observe equivalent. The two auth
    result fields, when an auth function's result sets them, are ignored with
    one `warn` per process. The `on_function_registration_function_id` hook input gains
    top-level `private` and `observable` (always sent) and `args_format`
    (when set); a result object may carry any of the three keys, and each one
    present replaces the registration's value.
11. **Codes.** An invalid `args_format` is `registrationrejected
    { code: INVALID_ARGS_FORMAT }`. Retired: `NOT_OWNER_DELIVERED`,
    `TARGET_MISMATCH`, `FILTERS_NOT_MATCHED`, `INVALID_OBSERVE_DESCRIPTOR`.
12. **Metrics.** `iii.observer.activations.total{delivery}` takes `engine`
    (engine-evaluated), `host` (reported by the host, local or routed) and
    `observer_call`. Telemetry heartbeat fields `trigger_count`,
    `trigger_types`, `trigger_registrations` become `observer_count`,
    `observable_function_count`, `private_function_count`.
13. **Call through an observer.** The SDK sends `invokefunction` with
    `function_id = obs.target` and `activation.observer_id`; the engine routes
    to the observer's stored target and echoes the frame's `function_id` on the
    result. No SDK-side pre-check: the engine is authoritative
    (`NOT_FUNCTION_OWNER`, `OBSERVER_NOT_FOUND`, `OBSERVER_NOT_ACTIVE`, policy).
    A call over `max_in_flight` fails with `OVERFLOW`. A fire-and-forget call
    (no `invocation_id`) that the engine refuses is dropped and counted under
    its code as the drop reason, with a `debug` log.
14. **`TriggerAction` is `CallAction`** in `engine/src/protocol.rs` and every
    SDK; the wire field `action` is unchanged.
15. **Trigger formats.** `engine/src/trigger_formats.rs` becomes
    `event_formats.rs` with only the event payload types engine-hosted sources
    use.

36. **What routed observers observe.** The engine takes a routed call's
    `output` from `invocationresult.result`, which is the handler's return
    value whenever iii delivers (a function with `onObserverDeliver` lists no
    `observed` ids). The engine strips `observed` before forwarding a result,
    and activates the `observed` ids on every routed answer from an
    `observe.v2` host, including a target's answer to an activation.
37. **0.25 `observe` object.** Decoded from any connection:
    `{ delivery?, args_format?, legacy_trigger_type?, allow? }`. `delivery:
    auto` and an absent `delivery` are accepted; `args_format` and
    `legacy_trigger_type` are ignored; `allow: false` means `observable:
    false`; `delivery: owner` or an unknown value is refused with
    `UNSUPPORTED`.

38. **No host reports for routed calls.** A host reports matches of a routed
    call only through `invocationresult.observed`; an `invocationsobserved`
    item always reports a local call of the reporting host. The engine drops an
    item whose `invocation_id` is a routed call it forwarded to that host, as
    `INVALID_REPORT`.
39. **Errors on routed answers.** `observed` may ride an `invocationresult`
    that carries `error`: iii's delivery rethrows the handler's error, and the
    envelope's `status` and `error` come from the answer.
40. **`_caller_worker_id`.** The engine removes the top-level
    `_caller_worker_id` from every observed `input` and `output` it delivers:
    the envelope's fields, and the bare value under `deliver: input` or
    `deliver: output`, whether it kept the value itself or a host reported it.
41. **Holding registrations.** An SDK sends a `registerfunction` that needs
    `observe.v2` (decision 18) only after the current connection's
    `workerregistered` advertised `observe.v2`, and decides again on every
    reconnect.
42. **Engine-hosted source caller.** The `caller` of a call to an
    engine-hosted source is `{ worker_name: "iii-engine" }`, as for
    activations.
43. **HTTP-invoked functions.** The engine evaluates the calls of a function
    registered with `invocation` (an HTTP endpoint) itself, as for
    engine-hosted functions, whatever the registering connection declared.
44. **Re-acks.** As in 0.25: an SDK re-acks every `functionregistered` entry,
    runs `onObserverAdded` only for observers it does not already hold, and
    treats an observer whose `args` changed as removed and then added.

45. **`onObserverMatch`.** Per function. A predicate runs once per active
    observer per call, before filters; a predicate that throws counts as no
    match and is logged once per function. Keys are computed once per observer
    when it is accepted and once per call; an observer whose key function
    throws is rejected (`OBSERVE_REJECTED`). A match hook set while observers
    are active rebuilds the index from them. With `onObserverDeliver` also set,
    the deliver hook receives the matched observers.

## SDKs (all five)

16. **Calling.** `call` (and `call_async` in Python, `Call` in Go) is the API;
    `CallRequest`, `CallAction`. Aliases: `trigger`, `trigger_async`,
    `Trigger`, `TriggerRequest`, `TriggerAction` — same function or type,
    marked deprecated where the language does it at no runtime cost (JSDoc
    `@deprecated`, Rust `#[deprecated]`, Go `// Deprecated:`, Python
    docstring). No runtime warning.
17. **Removed from every SDK:** `registerTrigger`, `registerTriggerType`,
    `unregisterTriggerType`, `TriggerHandler`, trigger handles, typed trigger
    configs (`IIITrigger`, `HttpTriggerConfig`, `CronTriggerConfig`, …),
    `EngineTriggers`, the `observe` registration descriptor, `Delivery`,
    `Observe::owner()/auto()`, `ObserveHandler`, `obs.emit`, `obs.call`,
    `Observer.legacy`. Engine source ids live in `EngineSources`
    (`FUNCTIONS_CHANGED`, `WORKERS_CHANGED`, `OBSERVERS_CHANGED`, `LOG`,
    `SPAN`, `CONFIGURATION_UPDATED`, `STREAM_CHANGED`, `STREAM_JOIN`,
    `STREAM_LEAVE`).
18. **Older engines.** Against an engine without `observe.v2`, functions that
    declare `args_format`, `private` or `observable: false`, or set
    `onObserverMatch` or `onObserverDeliver`, stay unregistered and the SDK logs `UNSUPPORTED` once
    per function. Everything else works, with the engine evaluating routed
    calls: the SDK delivers local calls only, sends no `observed` and no
    `output_is_input`.
19. **`Observer.accepted()`** resolves after the SDK has acked the observer.
    Calls through the observer sent after it resolves reach an active observer
    (frames are processed in order). Providers send initial state this way
    (ADE asset sync, ring-on-bind).
20. **`onObserverDeliver` context.** The hook runs inside the function's
    invocation context and span, so calls it makes have the function's
    invocation as caller. It receives the observers that matched
    (`onObserverMatch`, or all of them), as a snapshot taken when the call
    started. iii delivers nothing for a function that sets it.
21. **`standby`.** On `FUNCTION_NAMESPACE_CONFLICT` the SDK retries the
    registration every second (±20 % jitter) until it succeeds or the function
    is unregistered; hooks run when `functionregistered` arrives.
22. **Freezing private values.** Node freezes the input and result of a call
    to its own private function when `NODE_ENV === 'development'`. No other
    SDK freezes.
23. **Shorthand.** `observe('fn::id', target)`; Go adds
    `client.ObserveFunction(id, target, opts...)`.
24. **Per-SDK shapes.**
    - **Rust.** `Call { function_id, invocation_id, status, input: Value,
      output: Option<Value>, error, caller, metadata }`. Hooks take `Observer`
      by value. Traits `AddedHook::on_added`, `RemovedHook::on_removed`,
      `DeliverHook::on_deliver(call, observers) -> Result<Value, Error>`,
      implemented for async closures. `FunctionRef` methods take `&self` and
      return `&Self`; `clear_observer_added()`, `clear_observer_removed()`,
      `clear_observer_match()`, `clear_observer_deliver()` remove. Client:
      `on_observer_added(h)`, `on_observer_added_for(id, h)`, `…_removed…`,
      `on_observer_match_for(id, m)`, `on_observer_deliver_for(id, h)`.
      `is_observed()` and `is_observed_for(&call)`. `Match::predicate(f)` and `Match::keys(observer_fn, call_fn)` are the `onObserverMatch` forms; the SDK files observers in the key index in hook-task order.
      `RegisterFunction::new(h).args_format::<T>().private().observable(false)
      .standby()`.
    - **Node / browser.** As in the spec §4. `CallRequest`, `CallAction`,
      `ObserveRejection`, `EngineSources` exported from the package
      root.
    - **Python.** `register_function(id, handler, *, args_format=None,
      private=False, observable=True, standby=False, ...)` keyword options
      (existing keywords stay). Hook methods work as decorators and plain
      calls and return the hook; `iii.on_observer_added('id')` with no hook is
      a decorator factory; passing `None` as the hook removes it. Hooks may be sync or async. `on_observer_match` takes a predicate `(obs, call) -> bool`, or `observer=` and `call=` key functions. `Call` is a dataclass.
    - **Go.** `RegisterFunction` keeps its current return shape. Hooks are
      func types (`AddedHook func(ctx, *Observer) error`, …). `Call` struct
      with `Input`, `Output json.RawMessage`. `CallRequest` with `type
      TriggerRequest = CallRequest`; `CallAction`. `Observer.Matches(*Call)`.
      `client.OnObserverAdded(h)`, `client.OnObserverAddedFor(id, h)`,
      `client.OnObserverDeliverFor(id, h)`, `ref.OnObserverDeliver(h)`,
      `client.OnObserverMatchFor(id, m)` and `ref.OnObserverMatch(m)` with `iii.MatchFunc(func(*Observer, *Call) bool)` or `iii.MatchKeys(observerKeys, callKeys)`.

## CLI and crates

25. **`iii call`** is the subcommand; `trigger` and `t` are aliases, shown in
    help as aliases. Module `engine/src/cli_trigger/` becomes
    `engine/src/cli_call/`. Telemetry records `call` for every form.
26. **`compose-operation`** becomes `compose::on-operation-progress`, observed
    with filters on `input.operation_id` / `input.terminal` (events happen
    anyway: no args).
27. **Benches and load tests.** `trigger_fanout_bench` becomes
    `observe_fanout_bench`; `observe_dispatch/auto/*` becomes
    `observe_dispatch/default/*`. The loadtest `emit` scenario becomes
    `source`; a new `match-keys` scenario measures matching by keys. The §13 rows are
    measured by `sdk/packages/rust/iii/benches/observe_bench.rs` (runs on
    macOS) and by the loadtest on Linux when available.

## Workers

28. **Events that happen anyway are observed with filters; setup needs
    args.** Session-scoped events (session-manager, harness, approval-gate)
    take no args; consumers filter on `input.session_id`.
29. **Naming.** A trigger type `T` of worker `W` becomes `W::on-<event>`
    (`console::workspace::changed` → `console::workspace::on-changed`). ADE
    UI assets: `console::ui::asset` with args `{ kind: 'script' | 'style' |
    'module', path }`; browsers observe `console::ui::on-assets-changed`.
30. **Harness.** The agent tools `harness::observe` and `harness::unobserve`
    replace the intercepted `engine::register_trigger`. Stored bindings with
    a `trigger_type` are translated at boot through the table in spec §11;
    an untranslatable one is retired with an owner notice. Harness events
    become private functions `harness::on-<event>`. `harness::triggers::*`
    becomes `harness::observers::*`; new records use `observer_fired`, and
    readers accept `trigger_fired`. Deliveries use the `Observed` envelope.
    Observers carry `expires_in_ms` as a backstop; never `max_fires` (the
    harness counts fires itself).
31. **rbac-proxy.** Advertises `observe.v1`, `observe.v2`; forwards the
    observer frames both ways; allows `local-dispatch.v1` only when the
    session has no `middleware_function_id`. Removed trigger frames are
    dropped with the engine's warn-once rule. `engine::observers::list|get`
    become discovery overrides filtered by access.
32. **ADE console.** The Triggers screen becomes Observers (`ext:observers`,
    stored `ext:triggers` state migrates). The injectable-UI contract renames
    `host.functionTriggers` → `host.functionCalls` and role
    `function-trigger` → `function-call`; readers accept the old role in
    stored conversations.
33. **SDK pins.** Every worker pins the v2 SDK (`iii-sdk = "=0.26.0"`, npm
    `0.26.0`, Python `>=0.26.0`, Go `v0.26.0`). Local builds use a path
    override that is not committed.
34. **Out of scope:** the external `iii-hq/templates` repo.
35. **Versions.** The engine, every SDK, the CLI and the iii crates move to
    `0.26.0` on this branch.
