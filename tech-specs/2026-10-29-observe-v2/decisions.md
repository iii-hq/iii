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
   as for v1. Where filters run is decision 55: the engine evaluates the
   filters of every routed call, a host that declared `observe.v2` runs its
   functions' match and deliver hooks for them, and a host evaluates the
   calls it makes to its own functions.
3. **Routed completion.** `Invocation.sender` carries a completion struct:
   the `result` and, from a host that declared `observe.v2`, how the call is
   delivered (decision 55: every observer, the ids its match hook matched, or
   delivered by its deliver hook). The `InvocationResult` arm fills it; the
   routed hook evaluates the observers' `on` and filters against the input it
   kept (the routed hook keeps the input exactly when the function is
   observed, as in 0.25) and the answer.
4. **`output_is_input`** exists on `invocationsobserved` items only. An
   `invocationresult` always carries the result, which the engine already uses
   as `output`. SDK detection: Node, browser and Python compare by identity
   (`output === input`, `output is input`); Rust and Go compare the serialized
   bytes, and only when building a report item that carries both. For a local
   call of a function that is not private, the handler gets its own copy of
   the input: Node, browser and Python compare its return value by identity
   with that copy, which must still hold the same JSON data as the copy the
   delivery took at call start, and then deliver the call-start copy as the
   output. A private function's call passes the input by reference and is
   compared by reference. An SDK sets it only when the engine advertised
   `observe.v2`; to an older engine the item carries `output`.
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
    `on_trigger_type_registration_function_id` are removed. An `rbac` config
    that sets one of the two `on_trigger_*` keys without its replacement
    (`on_observer_registration_function_id`,
    `on_function_registration_function_id`) makes the engine refuse to
    start, with an error naming the replacement: the config relied on a gate
    that would be off. With the replacement set, the old key is ignored with
    one `warn` at load naming the replacement. A key set to `null` (a `${VAR:}` template whose variable is
    unset resolves to it) counts as unset, the removed key and the
    replacement alike. The two auth
    result fields, when an auth function's result sets them, are ignored with
    one `warn` per process. The `on_function_registration_function_id` hook input gains
    top-level `private` and `observable` (always sent) and `args_format`
    (when set); a result object may carry any of the three keys, and each one
    present replaces the registration's value.
11. **Codes.** An invalid `args_format` is `registrationrejected
    { code: INVALID_ARGS_FORMAT }`. Retired: `NOT_OWNER_DELIVERED`,
    `TARGET_MISMATCH`, `FILTERS_NOT_MATCHED`, `INVALID_OBSERVE_DESCRIPTOR`.
12. **Metrics.** `iii.observer.activations.total{delivery}` takes `engine`
    (the engine matched the call among every active observer), `host` (the
    host named the observer: a report of a local call, or its match hook's
    `matched` on a routed answer, decision 55) and `observer_call`. Telemetry heartbeat fields `trigger_count`,
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
    value whenever iii delivers (a function with `onObserverDeliver` answers
    `delivered: true`). The engine strips `matched` and `delivered` before
    forwarding a result, and reads them on every routed answer from an
    `observe.v2` host, including a target's answer to an activation
    (decision 55).
37. **0.25 `observe` object.** Decoded from any connection:
    `{ delivery?, args_format?, legacy_trigger_type?, allow? }`. `delivery:
    auto` and an absent `delivery` are accepted; `args_format` and
    `legacy_trigger_type` are ignored; `allow: false` means `observable:
    false`; `delivery: owner` or an unknown value is refused with
    `UNSUPPORTED`.

38. **No host reports for routed calls.** A host names the observers of a
    routed call only through `invocationresult.matched`; an
    `invocationsobserved` item always reports a local call of the reporting
    host. The engine drops an
    item whose `invocation_id` is a routed call it forwarded to that host, as
    `INVALID_REPORT`.
39. **Errors on routed answers.** `matched` may ride an `invocationresult`
    that carries `error`, and the engine evaluates the observers of an answer
    that carries `error` like any other: iii's delivery rethrows the handler's
    error, and the envelope's `status` and `error` come from the answer.
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
46. **A host's call of its own private function.** It skips RBAC middleware:
    the middleware's forward comes from another connection, which a private
    function refuses. `functionregistered` offers `local_dispatch` for a
    function the session hosts privately, also under middleware.
47. **Observers whose observing worker is away.** When the observing
    connection closes, its observers are `suspended` for
    `observers.observer_grace_ms`, and the engine withdraws each from its
    function's host (`observerremoved`, which runs its removed hooks). A
    `functionregistered` lists none of them. On a `Reattach` within the grace
    period the engine announces each again to its function's host
    (`observeradded`, in creation order per function), and it goes `active` on
    the ack. A host holds only observers that can become active, so
    `accepted()` resolving means a call through the observer reaches an active
    observer (decision 19). An observer that does not come back is disposed
    with nothing more sent to the host, as is one its worker replaces from a
    new connection (decision 51). A `Reattach` applies the creation rules of
    the new connection's session to each observer it re-parents, before the
    host hears of it: an observer whose function or target the session may
    not invoke, or that `rbac.on_observer_registration_function_id` denies,
    is disposed `FORBIDDEN`. The hook gets the registration as the worker
    asked for it (filters in normal form) and the new session's context; an
    object answer keeps the observer as it is, since a rewrite applies to a
    registration being created.
48. **The announced `capture`** (`observeradded`, `functionregistered`) also
    names the part `deliver` hands the target: `input` under `deliver: input`,
    `output` under `deliver: output`. A host's report of a local call carries
    the parts its matched observers' announced `capture` names, so the engine
    has what it delivers.
49. **A call through an observer runs as an activation of the observer.**
    Its target runs under the observing worker's session (none for an
    engine-owned observer), and the engine removes a top-level
    `_caller_worker_id` from its payload, as from every observed value it
    delivers (decision 40). The host chose the payload and the observing
    worker chose the target, so the host's authority reaches neither the
    target nor what the target calls. The host is the call's `caller`.
50. **Engine-hosted ids.** The engine hosts the sources it registers. A
    worker that registers one of their ids while the engine module that
    hosts it is running is refused (`FUNCTION_NAMESPACE_CONFLICT`, naming no
    owner); a worker that holds the id when the module starts (a config
    reload adds iii-stream) loses it with the same `registrationrejected`,
    and its teardown leaves the engine's source in place. `engine::workers::info`
    lists each source among the functions of the builtin worker that hosts it.
51. **A worker back on a new connection.** An `observe` that names the id of
    a suspended observer (decision 47) from a connection under the same
    worker name, in the same namespace, replaces it: the engine disposes the
    suspended observer, which its host has already let go (nothing more is
    sent to the host for it), and adds the new observer, announced to the
    host like any new observer. This is how a restarted worker re-observes
    under stable ids (queue consumers keep their backlog by id). From any
    other worker the id stays taken (`OBSERVER_ID_CONFLICT`) until the grace
    period ends. The worker name and namespace are the ones each connection
    declared on `engine::workers::register`, the pair the engine admits one
    live worker for (`WORKER_NAMESPACE_CONFLICT`); a connection that declared
    no name replaces nothing. The replaced observer is disposed with
    `OBSERVER_DISCONNECTED`, and the earlier connection, reattaching, has
    its replayed `observe` answered `disposed`, as for any observer disposed
    while its worker was away.
55. **Filters run where the system is fastest.** Args are the worker's and
    filters are iii's. The engine holds everything a routed call's filters
    read: its input (it forwarded the invocation and keeps the input when an
    observer needs it), its output or error (the `invocationresult`,
    decision 36), its caller and its metadata. So the engine evaluates the
    observers' `on` and filters of every routed call, at no extra frame, and
    the host keeps what is worker code: the function's match hook, which
    reads the observers' args, and its deliver hook. An `observe.v2` host's
    `invocationresult` to a routed call says which of three cases the call
    is:

    | The answer carries | The function | The engine |
    |---|---|---|
    | neither field | has no match hook and no deliver hook | evaluates every active observer of the function |
    | `"matched": [ids]` | has a match hook: the observers it matched, before `on` and filters, each once; `[]` when it matched none, or when the host held no active observer of the function as the call started (the hook then has none to match and does not run) | evaluates those of them that are active observers of the invoked function; any other id counts `INACTIVE` |
    | `"delivered": true` | has a deliver hook, which delivered the call; the answer is what the hook returned or threw | activates nothing |

    A host sends at most one of the two fields, and `delivered` outweighs
    `matched`. `matched` is absent when the function has no match hook; a
    function with a match hook and no deliver hook always answers with it.
    The engine reads both from `observe.v2` connections only, on every routed
    answer (decision 36), leniently (`null` is absent, a `matched` that is
    not an array is `[]`, a non-string id is skipped, and only `true` sets
    `delivered`), and strips both before forwarding a result. Only the
    function's host answers for its observers: an `observe.v2` answer from a
    connection that does not host the function (a caller answering its own
    call by its id, say) activates nothing, whichever case it carries, and
    is not evaluated. Each observer its `matched` names counts
    `REPORTER_NOT_OWNER`, and an answer without `matched` (neither field,
    or `delivered`) counts it for every active observer of the function. A
    routed call of an `observe.v2` host that ends without its answer
    activates nothing, and each of its observers counts a `CANCELLED` drop.
    Hosts without `observe.v2`, engine-hosted functions and HTTP-invoked
    functions (decision 43) are evaluated against every active observer.

    An answer too large to send is answered `payload_too_large`. That error
    is not the handler's outcome, so it activates no observer: to an
    `observe.v2` engine the host sends it with `delivered: true` when the
    deliver hook delivered the call, and with `matched: []` otherwise,
    whatever the function's hooks. An engine without `observe.v2` gets the
    bare error. The caller gets `payload_too_large` either way.

    When the engine has no room to evaluate a routed call (an evaluator's
    queue, or the bytes it holds for calls waiting to be evaluated, is
    full), it evaluates no filter for the call, and each observer the call
    could activate counts an `OVERFLOW` drop: every active observer of the
    function, or the ones the match hook named, whatever their filters. The
    engine counts the same for every call it evaluates, an `observe.v1`
    host's and an engine-hosted function's included.

    The engine never sees a host's calls of its own functions, so the host
    evaluates `on` and filters of its local calls and reports the observers
    delivered (`invocationsobserved`, decisions 4, 38, 48).

    The engine matches every evaluation through `iii_filter::FilterIndex`,
    and every SDK scans a small set of observers and matches through its
    equality index once that index narrows the set. `FilterIndex` files each
    observer under one `$eq` (or `$in`) condition of a scalar (number,
    string, boolean, `null`) on a path of its filter's top-level `$and`, path
    to value to observers, by the language's equality (numbers as doubles,
    `-0` as `0`), and keeps the observers without one in a list it scans. A
    call costs about one lookup per indexed path plus one filter evaluation
    per candidate. Where the lookups would spare a call little, the index
    evaluates every filter in key order, as a scan does: when it files fewer
    than two observers per indexed path, and when a call's lookups leave half
    the observers or more. A call therefore costs at most about a scan plus
    one lookup per indexed path. Its matches are exactly the observers a scan
    of every filter matches, in key order, proven by a differential test
    against the evaluator (the shared generator, an equality-heavy corpus,
    churn, every conformance vector and the diff corpus). It is persistent:
    the active-set version a call holds keeps its index, and an observer's
    change copies `O(log n)` nodes. A function none of whose observers has
    had an indexable filter (`Filter::is_indexable`) keeps no index: its
    calls scan the observers, which is what the index would do with them, so
    an unfiltered observer's change costs what it costs without an index; the
    first indexable observer builds the index from every observer, and it is
    kept up to date from then on.

    The SDKs match a local call the same way. Each evaluates every filter of
    a small set and matches a larger one through an index of the equality
    conditions by `FilterIndex`'s rule (the conditions the index serves, then
    the whole filter of each candidate), with the matches a scan yields,
    proven against its evaluator:

    | SDK | Evaluates every filter | Matches through the index |
    |---|---|---|
    | Rust | a set of at most 16 observers, or one more than half of whose observers accept the call's outcome and have no indexable filter | otherwise: `FilterIndex`, with its plan |
    | Node, browser | while no indexed path files more than one observer | otherwise: their own index |
    | Python | a call that evaluates at most 8 observers, and every call of a set until the set's calls have evaluated as many filters as it has observers | otherwise: the set's index, built once and kept |
    | Go | where `FilterIndex`'s plan would: fewer than two observers per indexed path, or lookups that leave half the observers or more | otherwise: its own index, merging the lists the lookups leave |

    Under decision 12, an activation of a routed call its host did not narrow
    is labelled `engine`, and one its match hook named `host`.
    `isObserved(call)`, `capture`, `deliver`, `on`, policy (`max_fires`,
    expiry, `max_in_flight`), the loop guard, the caller stamps (decisions
    40, 49), the reports of local calls, match keys and `for_observer`
    (decision 52) behave as their own decisions say, and every observer
    receives what it receives for every call.

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
    per function. Everything else works, with the engine evaluating every
    observer of a routed call: the SDK delivers local calls only, sends no
    `matched`, no `delivered` and no `output_is_input`.
19. **`Observer.accepted()`** resolves after the SDK has acked the observer.
    Calls through the observer sent after it resolves reach an active observer
    (frames are processed in order), and so does a call of its function for
    that observer alone. Providers send initial state with such a call
    (decision 52: ADE asset sync, ring-on-bind).
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
    - **Every SDK.** Observers go to the engine in the order the worker
      created them, on the first connection and on every reconnect, so the
      engine announces them to each function's host in that order.
    - **Late rejection (`obs.reject()`, a failed catch-up).** The observer's
      removed hooks run on its function's hook queue at once, connected or
      not, and run once: the engine sends no `observerremoved` for a
      rejection that answers an announcement it has not seen acked (a replay
      after a reconnect), and the `observerremoved` it sends for a live one
      runs none again. The observer stays held with its rejection, which a
      replay that lists it gets again.
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

52. **Calls for one observer.** A host sends one observer something of its
    own (its current state when the observer is added, or an event it
    missed) by calling its own function with a call that only that observer
    matches. The call carries metadata `{ for_observer: <observer id> }`, and
    the function's match hook gives such a call to that observer alone: a
    predicate compares `call.metadata.for_observer` with `obs.id`; key
    functions also file each observer under `"for_observer:<id>"` and give
    such a call that one key. iii delivers it like every call, with that
    observer's `deliver`, `capture`, filters, `on` and policy, so a target
    receives every event of the function in one shape. The host makes the
    call after `obs.accepted()` resolves (decision 19). A call through an
    observer (`iii.call({ observer, payload })`) belongs in a deliver hook,
    whose function documents what its targets receive.

    ```ts
    iii.onObserverAdded('tmux::on-changed', (obs) => {
      obs.accepted().then(() => iii.call({ function_id: 'tmux::on-changed', payload: stateFor(obs.args), metadata: { for_observer: obs.id }, action: CallAction.Void() }))
        .catch(() => undefined)   // a rejected observer gets nothing
    })
    iii.onObserverMatch('tmux::on-changed', (obs, call) =>
      call.metadata?.for_observer ? call.metadata.for_observer === obs.id : obs.args.session === call.input.session)
    ```

    Each SDK has its equivalent: Rust `CallRequest::new(id, payload)
    .metadata(json!({ "for_observer": obs.id() }))`, Python
    `{"function_id": id, "payload": payload, "metadata": {"for_observer": obs.id}}`,
    Go `iii.CallRequest{FunctionID: id, Payload: payload, Metadata: ...}`.
53. **A call through an observer takes no metadata.** Its target's metadata
    argument is the observer's registration metadata, as for iii's delivery.
    Every SDK refuses a request that names metadata before anything is sent,
    with its usual error for a bad request (Node and browser `Error`, Python
    `ValueError`, Rust `Error::Handler`, Go an `iii:` error) and the same
    message: "a call through an observer takes no metadata; its target
    receives the observer's metadata". `null` is no metadata (decision 54),
    so it is accepted. The engine reads no `metadata` from an `invokefunction`
    with `activation`, as it reads no namespace from it (decision 13). No SDK
    sends one, and refusing it would take a wire error code and a drop reason
    of its own.
54. **`null` metadata is no metadata.** A call's metadata is any JSON value.
    `null` (Python `None`, Rust `Value::Null`, Go `json.RawMessage("null")`)
    is the same as none on every path: each SDK normalizes it before local
    dispatch and before sending, so the handler, the match and deliver hooks
    and the report see none, and the `invokefunction` frame leaves the field
    out. An `invokefunction` that arrives with `"metadata": null` reaches the
    handler as none too. The engine already reads `null` as none
    (`InvokeFunction.metadata` is an `Option`).
56. **Observer ids in the SDK.** An observer's id is the client's: the `id`
    option, or a fresh UUID. A handle holds its id from its `observe` call
    until its `dispose()` is called or the engine disposes it.
    - **A held id is refused at the call.** An `observe` whose id a handle of
      this client holds, in any state, is refused locally: nothing is sent,
      and the holding handle and its observer are untouched. The refusal has
      code `OBSERVER_ID_CONFLICT`, the engine's code for an id another
      observer holds, and the message `observer id already in use: <id>`
      after the SDK's prefix (`observe: ` in Node, the browser and Python,
      none in Go and Rust). Node and the browser throw an
      `ObserveRejection`, Python raises one, and Go returns an
      `*ObserveRejectionError`, each with that code (Go's `Error()` reads
      `iii: observe rejected: OBSERVER_ID_CONFLICT: observer id already in
      use: <id>`, as for the engine's rejections). Rust's `observe`
      returns a handle: the handle it returns is in state `error` with that
      reason, `ready()` fails with it, and its `dispose()` sends nothing and
      leaves the holding handle alone.
    - **An id is free again at once.** A client may observe under an id as
      soon as the handle that held it is disposed (a queue consumer observes
      again under its stable id). The engine's answers name an observer by
      id only, and the order of the frames tells the answers apart, with no
      wire field: the engine reads one connection's messages in order, sends
      that connection its observe answers in the order it makes them,
      answers every `observe` with exactly one `observeresult`, and sends
      `observeresult` for nothing else. Each SDK therefore:
      1. keeps, per id, the handles whose `observe` it sent on the current
         connection, oldest first. A disposed handle stays listed until the
         engine's `disposed` for it arrives, a newer handle of the id has its
         `observeresult`, or the connection closes;
      2. gives an `observeresult` for the id to the oldest listed handle
         that has none yet;
      3. gives an `observerstatechanged` for the id to the newest listed
         handle that has its `observeresult`, and drops one that arrives
         before any has it: a reattached connection hears its observers
         resume before its replayed `observe`s are answered, and each
         replay's `observeresult` carries the current state;
      4. on a new connection, sends the `unobserve`s it queued while
         disconnected before it replays any `observe`, so the engine disposes
         the earlier observer before it reads the new one's `observe`. Sent
         later, the `unobserve` would dispose the new observer: a reattached
         earlier observer takes the new `observe` as its replay (or refuses
         it, `OBSERVER_ID_CONFLICT`), and a replacement (decision 51) holds
         the id by then.

      The engine's late `disposed` for the earlier observer, sent before the
      engine read the new `observe`, therefore reaches the earlier handle,
      and the new handle hears its own answers only.

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
    session has no `middleware_function_id`, or for a function the session
    hosts privately, whose own calls it forwards directly (decision 46). Removed trigger frames are
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
