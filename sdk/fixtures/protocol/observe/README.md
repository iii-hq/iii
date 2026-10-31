# observe wire fixtures

Golden JSON for every message and field that observe adds to the engine
protocol, as of observe v2: the 0.25 messages
(`tech-specs/2026-10-13-observe/protocol.md` §2) with the v2 changes
(`tech-specs/2026-10-29-observe-v2/README.md` §8 and §11, and the binding
implementation decisions in `decisions.md` next to it). Triggers are not part
of the protocol (v2 §11), so no fixture carries a trigger message.

**The fixtures are the wire contract. SDKs must decode and encode these
exactly.** Every SDK's test suite must decode each file into its message type
and encode it back to the same JSON value (key order and whitespace do not
matter; keys, values and the absence of keys do). The engine must run the same
round trip through its `Message` enum
(`engine/tests/protocol_observe_fixtures.rs`), so every file here must be a
shape the engine both accepts and produces. Two files, marked *engine-only*,
carry the 0.25 `observe` object that no v2 SDK sends, and that v2 SDK types
do not have (dec. 17). SDK suites skip the decode/encode round trip for these
two files and instead check that no `registerfunction` they build carries an
`observe` key; the engine runs the round trip on them like on every other
file. If a fixture looks inconsistent with the spec, report it; do not fork
the format in one SDK.

**Sources.** The *From* column cites **v2 §n** (a section of the observe v2
spec), **dec. n** (an item of its `decisions.md`, which wins over the spec)
and **0.25 §n** (a section of the 0.25 `protocol.md`).

## Rules every file follows

- **Tagging.** One JSON object per file, tagged by `type` (lowercase, the
  existing `#[serde(tag = "type", rename_all = "lowercase")]` scheme). The
  file name starts with the tag; anything after the first `-` names the
  variant.
- **Optional means absent.** An optional field that is not set is left out,
  never sent as `null`. Decoders must still accept `null` for it and read it
  as absent.
- **These defaults are left out.** Five fields are not sent at their
  default: `private: false` and `observable: true` on `registerfunction`,
  `delivered: false` on `invocationresult`, `output_is_input: false` on
  `invocationsobserved` items, `observe_depth: 0` on `invokefunction`.
  Decoders accept each of them sent explicitly. `matched` on
  `invocationresult` is absent when the host did not narrow the call's
  observers, and sent, `[]` included, when its match hook ran
  (`invocationresult-matched-none.json`). Other fields are sent as set,
  default or not: an `observe` carries its `source` and `target` fields as
  the caller set them (`observe.json` sends `on: success`, `deliver:
  envelope` and `namespace: default`), items of `invocationsobserved` always
  carry `observe_depth`, and `localinvocationstats` always carries `errors`
  and `duration_ms_sum` of each function and both arrays.
- **`null` is a value where it carries meaning.** In `invocationsobserved`
  items, `input`, `output`, `error` and `metadata` keep `null` apart from
  absent: `"output": null` is a function that returned `null`, while a
  missing `output` was not captured, does not exist, or equals `input`
  (`"output_is_input": true`). In `invocationresult`, `"result": null` is a
  function that returned `null`; `result` is absent when the function
  returned nothing, and on an answer that carries `error`.
- **Enums are lowercase strings.**
  `capture[]`: `input | output | error | caller | metadata`.
  `on`: `success | error | complete`. `deliver`: `envelope | input | output`.
  `state`: `pending | active | suspended | error | disposed`.
  `status`: `success | error`. Decoders must accept a value they do not know
  (a newer peer) without failing the frame, and keep it as an unknown value.
- **Observer defaults (0.25 §2.2).** Absent `on` is `success`, absent
  `capture` is `["input", "output", "caller"]`, absent `deliver` is
  `envelope`, absent `observe_depth` is `0`. `capture: []` is valid: the
  envelope then carries ids, status and timings only.
- **Unknown or invalid values are refused, never defaulted.** The engine
  answers an `observe` with an unknown `on` / `deliver` / `capture[]` value
  with `observeresult` error `UNSUPPORTED`, and a `registerfunction` whose
  `args_format` is not a valid JSON Schema with `registrationrejected`
  `INVALID_ARGS_FORMAT` (dec. 11).
- **Malformed new fields are not fatal.** The engine reads `caller`,
  `observe_depth` and `activation` on `invokefunction`, and `matched` and
  `delivered` on `invocationresult`, leniently: a bad value of the first
  three is treated as absent, a `matched` that is neither an array nor `null`
  as an empty list (a non-string id in it is skipped), and `delivered` as
  `true` only when it is `true`. The rest of the frame is handled (a caller
  still gets its result). It answers
  an `observe` it cannot decode but whose `id` is a string with
  `observeresult` error `INVALID_ARGS`, and an `invokefunction` it cannot
  decode but whose `invocation_id` is a UUID with an `invocationresult`
  error.
- **Ids are strings.** Observer ids are client-generated UUID v4 strings,
  stable across replays (0.25 §2.2). `invocation_id`, `activation_id`, the
  ids in `matched` (of an `invocationresult` and of an `invocationsobserved`
  item), and the ids inside `caller` are strings too.
- **Codes are exact strings** from 0.25 §4 as amended by v2 §8.5 and dec. 11
  and 20: error codes in `error.code` and `registrationrejected.code`, state
  reasons in `reason.code`, SDK drop reasons in `dropped_reports[].reason`
  (`BUFFER_FULL`, `TOO_LARGE`, `DISCONNECTED`; the engine counts any other
  SDK reason as `OTHER`). `NOT_OWNER_DELIVERED`,
  `TARGET_MISMATCH`, `FILTERS_NOT_MATCHED` and `INVALID_OBSERVE_DESCRIPTOR`
  are retired.
- **Numbers.** `revision`, `fires`, `count`, `errors`, `window_ms`,
  `started_at_ms`, `max_fires`, `expires_in_ms`, `max_in_flight` and
  `observe_depth` are non-negative integers. `duration_ms` and
  `duration_ms_sum` are floats (the fixtures use non-integral values so the
  round trip is unambiguous in every language).
- **Features (dec. 2, v2 §8.1).** The engine advertises `observe.v1`,
  `observe.v2` and `local-dispatch.v1` in `workerregistered.features`, in that
  order. Every v2 SDK declares `observe.v1`, `observe.v2` and
  `local-dispatch.v1` in the `engine::workers::register` payload; the native
  SDKs (Node, Python, Rust, Go) also declare `large-messages.v1`, the browser
  SDK does not. `observe.v2` includes `observe.v1`: host sync, `caller`,
  `observe_depth` and `activation` work as for `observe.v1`. A connection's
  features are fixed by the first `engine::workers::register` that lists any
  (at most 32 names of at most 64 bytes); later declarations are ignored.
- **Feature gate (0.25 §1).** Of the engine → worker messages below,
  `functionregistered`, `observeradded` and `observerremoved` go only to
  connections that declared `observe.v1` (v2 SDKs declare it too);
  `observeresult` and `observerstatechanged` go to the observing connection
  regardless; the others go to every connection. The engine adds `caller`,
  `observe_depth` and `activation` to an `invokefunction` only for
  connections that declared `observe.v1`. It decodes every worker → engine
  message from anyone.
- **Who delivers (v2 §8.2, dec. 2, 3, 20, 36, 38, 39, 43, 55).** A
  connection that declared `observe.v2` runs what is worker code for the
  calls of the functions it hosts: the handler, and the function's match and
  deliver hooks. It delivers its local calls itself and reports them
  (`invocationsobserved`; an item always reports a local call of the
  reporting host, Notes, caller on report items). Every call the engine
  routes to it carries an `invocation_id`, `void` calls included, and the
  host answers each with `invocationresult`: `result` is the handler's
  output, or `error` the error the handler threw, and the answer says how
  the call is delivered, in one of three ways:

  | The answer carries | When | The engine |
  |---|---|---|
  | neither `matched` nor `delivered` (`invocationresult-routed.json`) | the function has no match hook and no deliver hook | evaluates `on` and filters of every active observer of the function |
  | `matched`: observer ids, each once, `[]` when none (`invocationresult-matched.json`, `invocationresult-matched-error.json`, `invocationresult-matched-none.json`) | the function's `onObserverMatch` ran: these are the observers it matched, before `on` and filters | evaluates `on` and filters of those of them that are active observers of the invoked function, and counts any other id as `INACTIVE` |
  | `delivered: true` (`invocationresult-delivered.json`) | the function's `onObserverDeliver` delivered the call: the answer is what the hook returned or threw | activates nothing |

  A host sends at most one of the two; with both, `delivered` wins. The host
  evaluates no filter for a routed call: the engine already holds what the
  filters read, the input it kept when it forwarded the call, the caller it
  forwarded it with and its metadata, and the answer. It builds the envelope
  from them: `status` is `error` when the answer carries `error` and
  `success` otherwise, `error` is the answer's `error`, and `output` is its
  `result`. It reads `matched` and `delivered` only from `observe.v2`
  connections, on every routed answer, a target's answer to an activation or
  to a call through an observer included, and strips both before forwarding
  a result to a caller. A routed call whose answer never comes (its caller
  left, the host went away) activates nothing, and each of its observers
  counts a `CANCELLED` drop. The engine evaluates every active observer, as
  for an answer with neither field, for the routed calls of hosts without
  `observe.v2`, of engine-hosted functions, and of functions registered with
  `invocation` (HTTP-invoked: the engine calls the endpoint and receives the
  result, so no connection answers).
- **Older engines (dec. 4, 18, 41).** An SDK sends `matched` or `delivered`
  and sets `output_is_input` only when the engine advertised `observe.v2`. To an
  engine without it, the SDK delivers local calls only, and a report item
  carries `output` where it would set `output_is_input`. *Advertised* means
  in the current connection's `workerregistered`: features learned on an
  earlier connection do not count. An SDK sends a `registerfunction` that
  needs `observe.v2` (one with `args_format`, `private: true` or
  `observable: false`, or for a function with `onObserverMatch` or
  `onObserverDeliver`) only after this connection's `workerregistered` listed
  `observe.v2`, also when it replays registrations after a reconnect. Other
  registrations need not wait.

## The 0.25 `observe` object (dec. 37)

A 0.25 SDK describes a function's observers with an `observe` object on
`registerfunction`: `{ delivery?, args_format?, legacy_trigger_type?, allow? }`.
The engine decodes it from any connection. `delivery` is `auto | owner`:
`auto` and an absent `delivery` are accepted, while `owner` (dec. 1) or a
value the engine does not know is refused with `registrationrejected`
`UNSUPPORTED`.
`allow: false` registers the function as `observable: false`; absent `allow`
is `true`. `args_format` and `legacy_trigger_type` are decoded and ignored.
No v2 SDK sends it: against an engine without `observe.v2`, a v2 SDK leaves a
function with `observable: false` unregistered (dec. 18).

## Worker → engine

| File | Message | What it pins | From | Optional fields |
|---|---|---|---|---|
| `observe.json` | `observe` | Every field set: a `cron::on-tick` observer with args, filters, the facet (`capture`, `on`, `deliver`), policy and metadata. | v2 §3; 0.25 §2.2 | `source.namespace`, `source.args`, `source.filters`, `source.capture`, `source.on`, `source.deliver`, `target.namespace`, `metadata`, `max_fires`, `expires_in_ms`, `max_in_flight` |
| `observe-minimal.json` | `observe` | Only the required fields (`id`, `source.function_id`, `target.function_id`): the frame `observe('orders::archive', 'search::reindex-order')` sends. Everything else takes its default. | v2 §3; dec. 23 | as above |
| `observe-on-error.json` | `observe` | An observer of a function without args: `on: error`, a filter on `input`, a custom `capture`, `max_in_flight`, and the `metadata` its activations carry as their sidecar (`invokefunction-activation.json`). | v2 §3; 0.25 §2.2 | as above |
| `unobserve.json` | `unobserve` | Dispose an observer this connection owns. | 0.25 §2.2 | none |
| `observerack.json` | `observerack` | The host accepts an added observer once every `onObserverAdded` that applies has resolved, echoing `function_id`, `namespace`, `id` and `revision` from `observeradded`. | v2 §4.3, §8.4 | `namespace` (absent: the sending connection's namespace), `error` |
| `observerack-error.json` | `observerack` | The host rejects an added observer (here one announced at revision 15 whose expression has an invalid day-of-week field): `error` is the `ObserveRejection` its `onObserverAdded` threw (here `INVALID_CRON_EXPRESSION`), `OBSERVE_REJECTED` for any other error, or `OWNER_TIMEOUT` for a hook past 30 s. Also the shape of a late rejection (`obs.reject`). | v2 §4.3, §10.2 | as above |
| `invocationsobserved.json` | `invocationsobserved` | One batch from one connection, the `checkout` worker, with three local call reports: a call that failed (`status: error`, `error`); a call that returned `null` (`"output": null`), made outside any handler, so it has no `caller`; and a call whose `input` and `output` differ, with the call's `metadata` for an observer that captures it. The failed call and the third one are made inside the `checkout::confirm` handler, so their `caller` names that invocation. No item carries `caller.worker_id` or `caller.worker_name`: the engine sets both, here from the reporting connection, since every item reports a local call of that worker (Notes, caller on report items). Every item is at `observe_depth` 0. | v2 §8.2; dec. 38; 0.25 §2.2 | per item: `namespace`, `input`, `output`, `error`, `caller`, `metadata`, `traceparent`, `baggage`, `output_is_input` |
| `invocationsobserved-event-source.json` | `invocationsobserved` | One batch from the `database` worker: an event source's local call (`database::on-row-changed` returns the row change it is called with), matched by two observers and sent with `"output_is_input": true` and no `output`. It is made inside the `database::execute` call of `invokefunction-caller.json`, so its `caller` is that invocation, with that call's `caller` as `parent`, and its `observe_depth` is that call's, 0. | v2 §8.2, §10.1; dec. 4 | as above |
| `localinvocationstats.json` | `localinvocationstats` | The periodic counts of the `database` worker's local calls and SDK-side report drops; sent only when non-zero. Every field is sent, `errors: 0` included. `dropped_reports` holds a `BUFFER_FULL` drop of `database::on-row-changed` reports. `DISCONNECTED` counts items lost to a closed connection. | 0.25 §2.2 | none (a decoder reads an absent `errors` or `duration_ms_sum` as 0, and an absent array as empty) |
| `registerfunction.json` | `registerfunction` | A function with no v2 field set: no `args_format`, and `private: false` / `observable: true` left out as defaults. | v2 §4.1, §8.3 | `description`, `request_format`, `response_format`, `metadata`, `invocation`, `args_format`, `private`, `observable`, `observe` |
| `registerfunction-private.json` | `registerfunction` | An event source with every v2 field it sets: `cron::on-tick` with top-level `args_format` and `private: true`, its `response_format` equal to its `request_format` (the handler returns the tick); `observable` left out (`true`). Hooks and `standby` never travel. | v2 §4.1, §8.3, §10.2; dec. 24 | as above |
| `registerfunction-not-observable.json` | `registerfunction` | `observable: false`: every `observe` of the function fails with `NOT_OBSERVABLE`. | v2 §4.1, §8.3 | as above |
| `registerfunction-v1-owner.json` | `registerfunction` | *Engine-only.* A 0.25 SDK's owner-delivered `cron::on-tick`: the 0.25 `observe` object with `delivery: owner` and `args_format`. The engine answers with `registrationrejected-unsupported.json`. | dec. 1, 37; v2 §11 | `observe.delivery`, `observe.args_format`, `observe.legacy_trigger_type`, `observe.allow` |
| `registerfunction-v1-not-observable.json` | `registerfunction` | *Engine-only.* A 0.25 SDK's `observe: { delivery: auto, allow: false }`, as 0.25 SDKs send it. `delivery: auto` is accepted and ignored, and the engine registers the function as `observable: false`. | dec. 37; v2 §11 | as above |
| `invokefunction-observer-call.json` | `invokefunction` | A call through an observer (`iii.call({ observer, payload })`), request/response: `function_id` is the observer's `target`, `activation` carries `observer_id` only, no `namespace`, `caller` is the `http::on-request` invocation the deliver hook runs in, `observe_depth` left out (0). | v2 §4.6, §8.2, §10.3; dec. 13, 20 | `caller`, `observe_depth`, `activation` (and every pre-0.25 optional field) |
| `invokefunction-observer-call-namespace.json` | `invokefunction` | A call through an observer that carries a `namespace` anyway. SDKs send none; the engine routes to the observer's stored target and ignores it. | v2 §4.6; dec. 13 | as above |
| `invokefunction-observer-call-void.json` | `invokefunction` | A fire-and-forget call through an observer (`iii.call({ observer, payload, action: CallAction.Void() })`): no `invocation_id`, `action: void`, `function_id` the observer's `target`, `activation` with `observer_id` only. This one is made outside any handler, so it has no `caller`; from a deliver hook it carries the hook's invocation as its `caller`, as in `invokefunction-observer-call.json`. A provider sends a new observer its initial state as a call of its own function with `metadata: { for_observer: <observer id> }`, reported like any local call (decision 52). No answer comes back; a refused call is dropped and counted (Notes, calls through an observer). | v2 §4.6; dec. 13, 19, 52 | as above |
| `invokefunction-legacy.json` | `invokefunction` | A call from a pre-0.25 SDK, without `caller`, `observe_depth` or `activation`. It must keep decoding. | 0.25 §2.4 | as above |
| `invokefunction-workers-register.json` | `invokefunction` | A native v2 SDK's (here Node) fire-and-forget `engine::workers::register` call (no `invocation_id` on a `void` call), declaring `observe.v1`, `observe.v2`, `local-dispatch.v1` and `large-messages.v1` in `data.features`. The browser SDK sends the same list without `large-messages.v1`. Only `data.features` is normative: the other `data` keys show what the Node SDK sends and are not part of the observe contract. An unset `isolation` is left out, never sent as `null`. | dec. 2; v2 §8.1 | `data.features` (absent: a pre-0.25 SDK, no features) |
| `invocationresult-routed.json` | `invocationresult` | An `observe.v2` host's answer to a routed call of `orders::create`, a function with no match hook and no deliver hook: the `result`, which is the handler's output, and neither `matched` nor `delivered`. The engine evaluates `on` and filters of every active observer of `orders::create` against the input it kept and this `result`. | v2 §8.2; dec. 3, 36, 55 | `result` (absent: the function returned nothing), `matched` (absent: not narrowed), `delivered` (absent: `false`), `error`, `traceparent`, `baggage` |
| `invocationresult-matched.json` | `invocationresult` | An `observe.v2` host's answer to a routed call of `orders::create`, whose `onObserverMatch` matched two observers: the `result`, and in `matched` their ids. The engine evaluates `on` and filters of those two only. | v2 §4.5, §8.2; dec. 3, 36, 55 | as above |
| `invocationresult-matched-error.json` | `invocationresult` | An `observe.v2` host's answer to a routed call of `payments::charge` that threw: `error` is the error the handler threw and there is no `result`, and in `matched` the `on: error` observer of `observe-on-error.json`, which the function's match hook matched. The engine evaluates that observer's `on` and its filter on the input it kept, and builds its envelope with `status: error` and this `error`. A decoder keeps `matched` on an error answer. | v2 §4.4, §8.2; dec. 3, 36, 39, 55 | as above |
| `invocationresult-matched-none.json` | `invocationresult` | An `observe.v2` host's answer to a routed call of `orders::create` whose match hook matched no observer: `"matched": []`, sent, not left out (an absent `matched` would leave every observer to the engine). The engine activates nothing. | v2 §4.5, §8.2; dec. 55 | as above |
| `invocationresult-delivered.json` | `invocationresult` | An `observe.v2` host's answer to a routed call of `approvals::request`, whose `onObserverDeliver` delivered the call (it called its approver's target through the observer and returned the answer): `result` is what the hook returned, and `"delivered": true`. The engine activates nothing for the call. | v2 §4.6, §8.2; dec. 20, 55 | as above |

## Engine → worker

| File | Message | What it pins | From | Optional fields |
|---|---|---|---|---|
| `workerregistered.json` | `workerregistered` | This engine's `features`: `observe.v1`, `observe.v2`, `local-dispatch.v1`, in that order. | dec. 2; v2 §8.1 | `reattach_token`, `features` |
| `workerregistered-v1.json` | `workerregistered` | What a 0.25 engine sends: `observe.v1` and `local-dispatch.v1`, no `observe.v2`. A v2 SDK holds each `registerfunction` that needs `observe.v2` until the current connection's `workerregistered` arrives, whatever an earlier connection advertised. On this one it leaves each function that declares `args_format`, `private` or `observable: false`, or sets `onObserverMatch` or `onObserverDeliver`, unregistered, logs `UNSUPPORTED` once for it, and runs everything else. It delivers local calls only, sends no `matched` and no `delivered`, and sends `output` where it would set `output_is_input`. | dec. 4, 18, 41; v2 §11 | as above |
| `workerregistered-legacy.json` | `workerregistered` | What a pre-0.25 engine sends: no `features`, so `observe()` fails with `UNSUPPORTED` and nothing runs locally. | 0.25 §1 | as above |
| `registrationrejected-unsupported.json` | `registrationrejected` | The engine's answer to `registerfunction-v1-owner.json`: `code: UNSUPPORTED`, the function and its namespace, and `owner_worker_id: ""`, as on every rejection that is not a conflict. Only that registration is refused; the connection stays open. | dec. 1, 37; v2 §11 | `worker_name`, `function_id` |
| `registrationrejected-invalid-args-format.json` | `registrationrejected` | The answer to a `registerfunction` whose `args_format` is not a valid JSON Schema: `code: INVALID_ARGS_FORMAT`. | dec. 11 | as above |
| `invocationresult.json` | `invocationresult` | The engine's answer to `invokefunction-observer-call.json`: the result the target returned for `invokefunction-observer-call-forwarded.json`, under the frame's `invocation_id` and `function_id`, without `matched` or `delivered`. The engine reads those from the target's host's answer, delivers that call to the target's own observers as they say, and strips both before forwarding. | dec. 13, 36, 55; v2 §4.6 | `result`, `matched`, `delivered`, `error`, `traceparent`, `baggage` |
| `invocationresult-not-function-owner.json` | `invocationresult` | The engine's refusal of `invokefunction-observer-call-namespace.json` from a connection that does not host the observer's function: `error.code: NOT_FUNCTION_OWNER` under the frame's `invocation_id` and `function_id`, no `result`, no `matched`, no `delivered`. Every refusal of a request/response call through an observer has this shape. | v2 §4.6, §8.5; dec. 13 | as above |
| `observeresult.json` | `observeresult` | The reply to `observe` (also to a replay): the observer's current state. | 0.25 §2.3 | `error`, `warnings` |
| `observeresult-error.json` | `observeresult` | A rejected observer: `state: error`, the `error`, and `warnings` from filter paths that could not be verified. | 0.25 §2.3 | as above |
| `observerstatechanged.json` | `observerstatechanged` | A lifecycle transition (here to `active`). `fires` is always sent. | 0.25 §2.3, §3.3 | `reason` |
| `observerstatechanged-reason.json` | `observerstatechanged` | A transition with its `reason`: the `HTTP_ROUTE_CONFLICT` that `http::on-request`'s `onObserverAdded` threw for a route `GET /users/:uid` next to `GET /users/:id`. | v2 §10.3; 0.25 §2.3 | as above |
| `functionregistered.json` | `functionregistered` | The `registerfunction` ack with the current observer set, less the observers whose observing worker is away (each is announced with its own `observeradded` when that worker reattaches, dec. 47); `local_dispatch: true`. `revision` is per `(namespace, function)` and goes up by one with every announcement: this message, each `observeradded` and each `observerremoved` (so an ack for an earlier announcement is stale). Always accepted; it replaces the set the SDK holds, also for a standby that takes over the function. Each entry has the observer shape of `observeradded.json`: never `deliver`, metadata, policy, the observing worker or the 0.25 `legacy` key. | v2 §7, §8.4; 0.25 §2.3 | none |
| `functionregistered-empty.json` | `functionregistered` | No observers yet, and `local_dispatch: false` (e.g. the session has RBAC middleware). `observers` is sent even when empty. `revision` is 1: the first announcement of the function. | v2 §8.4; 0.25 §2.3 | none |
| `observeradded.json` | `observeradded` | One observer attached to a function this connection hosts, as the host sees it: `id`, `args` (`{}` when none), the normalized `filters`, the `target` function id (its namespace is not disclosed), `capture` and `on`, always sent resolved (`capture` also names the part `deliver` hands the target, dec. 48); never `deliver`, metadata, policy, the observing worker or the 0.25 `legacy` key (gone with the legacy shim). | v2 §4.7, §8.4; 0.25 §2.3 | none |
| `observerremoved.json` | `observerremoved` | One observer detached: unobserved, disposed, rejected late, or its observing worker disconnected (dec. 47). An SDK ignores an added/removed message whose `revision` is not greater than the one it holds. | v2 §8.4; 0.25 §2.3 | none |
| `invokefunction-activation.json` | `invokefunction` | An activation: `data` is the `Observed` envelope (per the observer's `capture`; the observed `input` is the payload as its caller sent it, without the engine's `_caller_worker_id` stamp, dec. 40), `metadata` is the observer's sidecar, `caller` is the engine (`worker_name: "iii-engine"`, the observed function and invocation), `observe_depth` is the observed depth + 1, `activation` carries `observer_id` and `activation_id`. The envelope is built from `invocationsobserved.json`'s failed `payments::charge` call, whose `caller` the engine stamped with the `checkout` worker that reported it. It reaches the target with an `invocation_id` and no `action`; the target's SDK answers with an `invocationresult`. The engine discards that result for the observed side, and delivers the call to the target's own observers as for any routed answer (as its `matched` and `delivered` say, when its host declared `observe.v2`). The answer frees the observer's `max_in_flight` slot (so does `observers.activation_timeout_ms`, 30 s by default, for an activation never answered). | v2 §4.4; dec. 36; 0.25 §3.4 | `caller`, `observe_depth`, `activation` |
| `invokefunction-activation-engine-source.json` | `invokefunction` | An activation from an engine-hosted source: `engine::on-observers-changed`, called once per observer state change with the event `{ observer_id, function_id, target, state, reason? }` (here the `HTTP_ROUTE_CONFLICT` of `observerstatechanged-reason.json`). The observer, `0ad9be10` of `console::observers::refresh`, sets no `capture`, so it captures the default `input`, `output` and `caller`. The source's handler returns its event, so the envelope carries it as both `input` and `output`, and its `caller` is `{ worker_name: "iii-engine" }`, the caller of every call of an engine-hosted source (Notes, engine-hosted sources). The activation's own `caller` is the engine with the source's `function_id` and `invocation_id`, as for every activation. | dec. 6, 7, 42 | as above |
| `invokefunction-caller.json` | `invokefunction` | A routed call forwarded by the engine: `billing::close-invoice` calls `database::execute`. `caller.worker_id` / `worker_name` are stamped from the calling connection; `function_id` / `invocation_id` / `parent` are asserted by the calling SDK. `data` carries the engine's `_caller_worker_id` stamp, `billing`'s `worker_id`, as on every call a worker makes (Notes, `_caller_worker_id`). `observe_depth` is the calling handler's depth, here 0 and so left out: `billing::close-invoice` was called directly by `checkout::confirm`, and depth grows only on activations. To an `observe.v2` host it always carries an `invocation_id`. | v2 §8.2; dec. 40; 0.25 §2.4 | as above |
| `invokefunction-observer-call-forwarded.json` | `invokefunction` | The engine's call to the target of a call through an observer: `invokefunction-observer-call.json` as `users::get`'s host receives it, under that frame's `invocation_id`. `function_id` is the observer's stored target, with no `namespace`. `data` is the payload the `http` worker sent, not an envelope, less a top-level `_caller_worker_id`: the call runs as an activation of the observer, under the observing worker's session (dec. 49). `caller` is the calling worker: `worker_id` and `worker_name` stamped from its connection, and the `function_id` and `invocation_id` it asserted (the `http::on-request` invocation its deliver hook runs in). `observe_depth` is the calling frame's depth + 1, here 1. `activation` carries the `observer_id` and the `activation_id` the engine assigned. `metadata` is the observer's metadata sidecar. The target's host answers with an `invocationresult`; the engine delivers that call to `users::get`'s own observers as the answer's `matched` and `delivered` say, strips both and forwards the result to the caller (`invocationresult.json`). The answer frees the observer's `max_in_flight` slot. | v2 §4.6, §8.2; dec. 13, 36; 0.25 §3.5 | as above |

## Notes for SDK authors

- `caller.parent` is one level only: a `parent` inside `parent` is not part of
  the format, and the engine drops it.
- The `filters` inside `functionregistered` / `observeradded` are the
  observer's filters in normal form (filters.md §5): `{}` matches every call,
  anything else is `{"$and": [...]}` at every level. Compile them as given.
- An `observe` without `filters` (or with `filters: null`) matches every call.
- **Acks (v2 §4.3).** A host acks every observer it is told about, each with
  the `revision` of the message that told it: every entry of
  `functionregistered.observers` (also the ones it already held before a
  re-registration: a new registration starts a new host relationship) and
  every `observeradded`. It sends the ack once every `onObserverAdded` that
  applies to the observer has resolved. Until then the observer stays
  `pending`; after `observers.owner_ack_timeout_ms` (30 s by default) it fails
  with `OWNER_TIMEOUT`. An ack whose `revision` is older than the
  announcement is ignored.
- **Hooks on `functionregistered` (0.25 §2.3; v2 §4.3, §7; dec. 44).** The
  message replaces the set of observers the SDK holds for the function. An
  entry the SDK already holds with the same `args` is re-acked without
  running any hook. A new entry runs `onObserverAdded` and is acked once it
  resolves. A held observer the message does not list is removed:
  `onObserverRemoved` runs if it was accepted. An entry whose `args` changed
  counts as removed, then added. A standby taking over a function holds none
  of its observers, so it runs `onObserverAdded` for every entry.
- `invocationsobserved` items are checked one by one. An item that does not
  decode, or whose `status` is neither `success` nor `error`, is dropped alone
  (counted as `INVALID_REPORT` in `iii.observer.dropped.total`) and the rest of
  the batch is applied.
- **Caller on report items (dec. 38).** An SDK sends an item's `caller`
  with the `function_id`, `invocation_id` and `parent` of the call's context,
  never `caller.worker_id` or `caller.worker_name` (a `parent` keeps its own,
  as the context received them), and sends no `caller` for a call made
  outside any handler. Every item reports a local call of the reporting host,
  whose worker made the call: the engine stamps `caller.worker_id` and
  `caller.worker_name` from the reporting connection on every item it
  applies, overwriting any that were sent, and creates `caller` when it is
  absent. An item whose `invocation_id` is a call the engine routed to the
  reporting connection is dropped as `INVALID_REPORT`, before that call's
  answer or after it. Items an SDK loses to a closed connection count in
  `localinvocationstats.dropped_reports` as `DISCONNECTED`.
- **`_caller_worker_id` (dec. 40, 49; 0.25 §2.4).** The engine adds
  `_caller_worker_id` to the `data` of every call a worker makes, calls routed
  to `observe.v2` hosts included (`invokefunction-caller.json`). A call
  through an observer runs as an activation of the observer, and the engine
  removes a top-level `_caller_worker_id` from its payload
  (`invokefunction-observer-call-forwarded.json`).
  SDKs pass `data` to the handler, to `onObserverDeliver` and to filters as it
  arrived, stamp included, so a handler that returns its input returns the
  stamp too. No observed value an observer receives contains it: the engine
  removes a top-level `_caller_worker_id` from every observed `input` and
  `output` it delivers, both the envelope's fields and the bare value that is
  an activation's `data` under `deliver: input` or `deliver: output`,
  wherever it took them from (the input it kept for a routed call, an
  `invocationresult`'s `result`, an item's `input` or `output`). It removes
  it from an item's `input` before it fills `output` from `input` for
  `output_is_input`. The `result` the engine forwards to the caller stays as
  the host sent it.
- **Engine-hosted sources (dec. 7, 42).** The `caller` of a call of an
  engine-hosted source is `{ worker_name: "iii-engine" }`, with no
  `worker_id`, `function_id` or `invocation_id`. An observer that captures
  `caller` gets it in its envelope, and a filter on `caller.worker_name` sees
  `"iii-engine"` (`invokefunction-activation-engine-source.json`). The
  activation's own `caller` is the engine with the source's `function_id` and
  `invocation_id`, as for every activation.
- **`matched` (v2 §4.5, §8.2, dec. 4, 36, 55).** iii's delivery of a call
  delivers each observer that `onObserverMatch` selects (every observer by
  default) whose `on` and filters match the call, once. For a local call the
  host evaluates `on` and filters and lists the observers delivered in its
  item's `matched`; the engine activates each. For a routed call the host
  lists in its answer's `matched` the observers its match hook selected,
  before `on` and filters, which the engine evaluates; a function without a
  match hook sends no `matched`. A report item that would carry both `input`
  and `output` of a handler that returned its input carries `input` and
  `"output_is_input": true` instead of `output`, and the engine fills
  `output` from `input`. Node, browser and
  Python detect it by identity, Rust and Go by comparing the serialized
  bytes. An `invocationresult` has no such flag: the engine takes `output`
  from its `result` and `error` from its `error`.
- **Calls through an observer (v2 §4.6, §8.5, dec. 13).** The engine is
  authoritative: it checks that the observer exists, that the sender hosts
  its function and that it is active, then applies policy. A request/response
  call gets a failure as its `invocationresult` `error`
  (`invocationresult-not-function-owner.json`): `OBSERVER_NOT_FOUND`,
  `NOT_FUNCTION_OWNER`, `OBSERVER_NOT_ACTIVE`, `OVERFLOW`, `LOOP_GUARD`, or
  `TIMEOUT` when the target does not answer within
  `observers.activation_timeout_ms`. A call over the observer's
  `max_in_flight` fails with `OVERFLOW`; it is not queued. A fire-and-forget
  call (`invokefunction-observer-call-void.json`) gets no answer: the engine
  drops a refused one, counts it in `iii.observer.dropped.total` with its code
  as the reason, and logs it at `debug`.
- **Private functions (v2 §6).** The engine answers an `invokefunction` of a
  `private` function from any connection but its host with an
  `invocationresult` error `FORBIDDEN`, and an `observe` whose target is
  private with `observeresult` error `FORBIDDEN`. A call the engine makes
  itself (a hook or middleware) to a private function also fails with
  `FORBIDDEN`.
- **Builtin payloads (dec. 6).** These are the inputs and results of engine
  functions, not frames, so no fixture pins them; an SDK that decodes them
  into types follows this note. Every function row, in
  `engine::functions::list`, `engine::functions::info` and the
  `engine::on-functions-changed` event, carries `observer_count` always,
  `args_format` when set, `private: true` when the function is private and
  `observable: false` when it is not observable, and no
  `registered_triggers`. `engine::workers::info` carries `functions` and
  `observers`, the ids of the observers the worker created, and no
  `trigger_types` or `registered_triggers`. `engine::register_trigger`,
  `engine::unregister_trigger`, `engine::triggers::list|info` and
  `engine::registered-triggers::list|info` are removed. The
  `engine::functions::list` row of `registerfunction-private.json`'s
  `cron::on-tick`, with the two observers of `functionregistered.json`:

  ```json
  {
    "function_id": "cron::on-tick",
    "namespace": "default",
    "worker_name": "cron",
    "description": "One call per tick of a cron expression. Observe it with args { expression }.",
    "args_format": {
      "type": "object",
      "additionalProperties": false,
      "required": ["expression"],
      "properties": { "expression": { "type": "string" } }
    },
    "private": true,
    "observer_count": 2
  }
  ```
- **Registration hook (dec. 10).** The RBAC
  `on_function_registration_function_id` hook is a function the engine calls,
  so its input and result are payloads, not frames. Its input carries, next
  to `function_id`, `description`, `metadata`, `namespace` and `context`, the
  registration's top-level `private` and `observable`, always sent (defaults
  included), and `args_format` when the registration sets it. A result object
  may carry any of `private`, `observable` and `args_format`; each one present
  replaces the registration's value.
- **Removed trigger frames (dec. 5).** The engine drops each of the five 0.25
  trigger frames with one `warn` per connection and message type. No v2 SDK
  sends them.
- Unknown message types and unknown fields must be ignored, as today.
