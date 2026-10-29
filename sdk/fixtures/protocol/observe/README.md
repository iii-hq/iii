# observe wire fixtures

Golden JSON for every message and field that observe adds to the engine
protocol, as of observe v2: the 0.25 messages
(`tech-specs/2026-10-13-observe/protocol.md` §2) with the v2 changes
(`tech-specs/2026-10-29-observe-v2/README.md` §8 and §11, and the binding
implementation decisions in `decisions.md` next to it). Triggers are not part
of the protocol (v2 §11), so no fixture carries a trigger message.

**The fixtures are the wire contract. SDKs must decode and encode these
exactly.** Every SDK's test suite decodes each file into its message type and
encodes it back to the same JSON value (key order and whitespace do not
matter; keys, values and the absence of keys do). The engine runs the same
round trip through its `Message` enum
(`engine/tests/protocol_observe_fixtures.rs`), so a file that is here is a
shape the engine both accepts and produces. Two files, marked *engine-only*,
carry the 0.25 `observe` object that no v2 SDK sends; an SDK suite exercises
them by checking that no `registerfunction` it builds carries an `observe`
key. If a fixture looks inconsistent with the spec, report it; do not fork the
format in one SDK.

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
- **Defaults are left out.** A field at its default is not sent:
  `private: false` and `observable: true` on `registerfunction`,
  `observed: []` on `invocationresult`, `output_is_input: false` on
  `invocationsobserved` items, `observe_depth: 0` on `invokefunction`.
  Decoders accept each of them sent explicitly. Items of
  `invocationsobserved` always carry `observe_depth`.
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
  `observe_depth` and `activation` on `invokefunction`, and `observed` on
  `invocationresult`, leniently: a bad value is treated as absent, and the
  rest of the frame is handled (a caller still gets its result). It answers
  an `observe` it cannot decode but whose `id` is a string with
  `observeresult` error `INVALID_ARGS`, and an `invokefunction` it cannot
  decode but whose `invocation_id` is a UUID with an `invocationresult`
  error.
- **Ids are strings.** Observer ids are client-generated UUID v4 strings,
  stable across replays (0.25 §2.2). `invocation_id`, `activation_id`, the
  ids in `observed` and `matched`, and the ids inside `caller` are strings
  too.
- **Codes are exact strings** from 0.25 §4 as amended by v2 §8.5 and dec. 11:
  error codes in `error.code` and `registrationrejected.code`, state reasons
  in `reason.code`, drop reasons in `dropped_reports[].reason`
  (`BUFFER_FULL`, `TOO_LARGE`). `NOT_OWNER_DELIVERED`, `TARGET_MISMATCH`,
  `FILTERS_NOT_MATCHED` and `INVALID_OBSERVE_DESCRIPTOR` are retired.
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
- **Who delivers (v2 §8.2, dec. 2, 3, 36).** A connection that declared
  `observe.v2` delivers the calls of the functions it hosts. Every call the
  engine routes to it carries an `invocation_id`, `void` calls included, and
  the host answers each with `invocationresult`. The host lists in `observed`
  the observers its delivery matched when that answer's `result` is the
  handler's output and `next` was given the call unchanged (by identity in
  Node, browser and Python; by serialized bytes in Rust and Go). Otherwise it
  leaves `observed` out and reports those matches in an `invocationsobserved`
  item for the same `invocation_id`, carrying the call `next` was given
  (`invocationresult-hook-result.json` with `invocationsobserved-routed.json`).
  The engine evaluates no filters for these calls: it activates each id in
  `observed` after checking that it is an active observer of the invoked
  function, and drops any other id. It reads `observed` only from `observe.v2`
  connections, on every routed answer, a target's answer to an activation or
  to a call through an observer included, and strips it before forwarding a
  result to a caller. For hosts without `observe.v2`, and for engine-hosted
  functions, the engine evaluates routed calls itself.
- **Older engines (dec. 4, 18).** An SDK lists ids in `observed` and sets
  `output_is_input` only when the engine advertised `observe.v2`. To an
  engine without it, the SDK delivers local calls only, and a report item
  carries `output` where it would set `output_is_input`.

## The 0.25 `observe` object (dec. 1, 37; v2 §8.3)

A 0.25 SDK describes a function's observers with an `observe` object on
`registerfunction`: `{ delivery?, args_format?, legacy_trigger_type?, allow? }`.
The engine decodes it from any connection. `delivery` is `auto | owner`:
`auto` and an absent `delivery` are accepted, while `owner` or a value the
engine does not know is refused with `registrationrejected` `UNSUPPORTED`.
`allow: false` registers the function as `observable: false`; absent `allow`
is `true`. `args_format` and `legacy_trigger_type` are decoded and ignored.
No v2 SDK sends it: against an engine without `observe.v2`, a v2 SDK leaves a
function with `observable: false` unregistered (dec. 18).

## Worker → engine

| File | Message | What it pins | From | Optional fields |
|---|---|---|---|---|
| `observe.json` | `observe` | Every field set: a `cron::on-tick` observer with args, filters, the facet (`capture`, `on`, `deliver`), policy and metadata. | v2 §3; 0.25 §2.2 | `source.namespace`, `source.args`, `source.filters`, `source.capture`, `source.on`, `source.deliver`, `target.namespace`, `metadata`, `max_fires`, `expires_in_ms`, `max_in_flight` |
| `observe-minimal.json` | `observe` | Only the required fields (`id`, `source.function_id`, `target.function_id`): the frame `observe('orders::archive', 'search::reindex-order')` sends. Everything else takes its default. | v2 §3; dec. 23 | as above |
| `observe-on-error.json` | `observe` | An observer of a function without args: `on: error`, a filter on `input`, a custom `capture`, `max_in_flight`. | v2 §3; 0.25 §2.2 | as above |
| `unobserve.json` | `unobserve` | Dispose an observer this connection owns. | 0.25 §2.2 | none |
| `observerack.json` | `observerack` | The host accepts an added observer once every `onObserverAdded` that applies has resolved, echoing `function_id`, `namespace`, `id` and `revision` from `observeradded`. | v2 §4.3, §8.4 | `namespace` (absent: the sending connection's namespace), `error` |
| `observerack-error.json` | `observerack` | The host rejects it: `error` is the `ObserveRejection` its `onObserverAdded` threw (here `INVALID_CRON_EXPRESSION`), `OBSERVE_REJECTED` for any other error, or `OWNER_TIMEOUT` for a hook past 30 s. Also the shape of a late rejection (`obs.reject`). | v2 §4.3, §10.2 | as above |
| `invocationsobserved.json` | `invocationsobserved` | One batch of four local call reports: a call that failed (`status: error`, `error`); a call that returned `null` (`"output": null`); a call whose `input` and `output` differ, with the call's `metadata` for an observer that captures it; and an event source's call (`database::on-row-changed` returns the row change it is called with) made inside the `database::execute` call of `invokefunction-caller.json`, so its `caller` is that invocation with its `parent` and its `observe_depth` is 2, matched by two observers and sent with `"output_is_input": true` and no `output`. | v2 §8.2, §10.1; dec. 4 | per item: `namespace`, `input`, `output`, `error`, `caller`, `metadata`, `traceparent`, `baggage`, `output_is_input` |
| `invocationsobserved-routed.json` | `invocationsobserved` | The matches of a routed call, reported by its `observe.v2` host under the routed `invocation_id`: the `orders::submit` call that `invocationresult-hook-result.json` answers. Its `onObserverDeliver` returned something other than the handler's output, so the item carries the call `next` was given, with the handler's `output`. `caller` is the one the routed call arrived with. | dec. 36; v2 §8.2 | as above |
| `localinvocationstats.json` | `localinvocationstats` | The periodic counts of local calls and SDK-side report drops; sent only when non-zero. | 0.25 §2.2 | `functions[].errors`, `functions[].duration_ms_sum` (default 0); both arrays default to empty |
| `registerfunction.json` | `registerfunction` | A function with no v2 field set: no `args_format`, and `private: false` / `observable: true` left out as defaults. | v2 §4.1, §8.3 | `description`, `request_format`, `response_format`, `metadata`, `invocation`, `args_format`, `private`, `observable`, `observe` |
| `registerfunction-private.json` | `registerfunction` | An event source with every v2 field it sets: `cron::on-tick` with top-level `args_format` and `private: true`, its `response_format` equal to its `request_format` (the handler returns the tick); `observable` left out (`true`). Hooks and `standby` never travel. | v2 §4.1, §8.3, §10.2; dec. 24 | as above |
| `registerfunction-not-observable.json` | `registerfunction` | `observable: false`: every `observe` of the function fails with `NOT_OBSERVABLE`. | v2 §4.1, §8.3 | as above |
| `registerfunction-v1-owner.json` | `registerfunction` | *Engine-only.* A 0.25 SDK's owner-delivered `cron::on-tick`: the 0.25 `observe` object with `delivery: owner` and `args_format`. The engine answers with `registrationrejected-unsupported.json`. | dec. 1, 37; v2 §11 | `observe.delivery`, `observe.args_format`, `observe.legacy_trigger_type`, `observe.allow` |
| `registerfunction-v1-not-observable.json` | `registerfunction` | *Engine-only.* A 0.25 SDK's `observe: { delivery: auto, allow: false }`, as 0.25 SDKs send it. `delivery: auto` is accepted and ignored, and the engine registers the function as `observable: false`. | dec. 37; v2 §8.3 | as above |
| `invokefunction-observer-call.json` | `invokefunction` | A call through an observer (`iii.call({ observer, payload })`), request/response: `function_id` is the observer's `target`, `activation` carries `observer_id` only, no `namespace`, `caller` is the `http::on-request` invocation the deliver hook runs in, `observe_depth` left out (0). | v2 §4.6, §8.2, §10.3; dec. 13, 20 | `caller`, `observe_depth`, `activation` (and every pre-0.25 optional field) |
| `invokefunction-observer-call-namespace.json` | `invokefunction` | A call through an observer that carries a `namespace` anyway. SDKs send none; the engine routes to the observer's stored target and ignores it. | v2 §4.6; dec. 13 | as above |
| `invokefunction-observer-call-void.json` | `invokefunction` | A fire-and-forget call through an observer (`iii.call({ observer, payload, action: CallAction.Void() })`): no `invocation_id`, `action: void`, `function_id` the observer's `target`, `activation` with `observer_id` only. Here a provider sends a new observer its initial state once `obs.accepted()` resolves; the call is made outside any handler, so it has no `caller`. No answer comes back; a refused call is dropped and counted (Notes, calls through an observer). | v2 §4.6; dec. 13, 19 | as above |
| `invokefunction-legacy.json` | `invokefunction` | A call from a pre-0.25 SDK, without `caller`, `observe_depth` or `activation`. It must keep decoding. | 0.25 §2.4 | as above |
| `invokefunction-workers-register.json` | `invokefunction` | A native v2 SDK's (here Node) fire-and-forget `engine::workers::register` call (no `invocation_id` on a `void` call), declaring `observe.v1`, `observe.v2`, `local-dispatch.v1` and `large-messages.v1` in `data.features`. The browser SDK sends the same list without `large-messages.v1`. | dec. 2; v2 §8.1 | `data.features` (absent: a pre-0.25 SDK, no features) |
| `invocationresult-observed.json` | `invocationresult` | An `observe.v2` host's answer to a routed call of `orders::create`: the `result`, which is the handler's output, and in `observed` the ids of the two observers its delivery matched. | v2 §8.2; dec. 3, 36 | `result` (absent: the function returned nothing), `observed` (absent: none), `error`, `traceparent`, `baggage` |
| `invocationresult-observed-error.json` | `invocationresult` | An `observe.v2` host's answer to a routed call of `payments::charge` that threw: `error` and no `result`, and in `observed` the `on: error` observer of `observe-on-error.json`, which its delivery matched. A decoder keeps `observed` on an error answer. | v2 §4.4, §8.2; dec. 3 | as above |
| `invocationresult-hook-result.json` | `invocationresult` | An `observe.v2` host's answer to a routed call of `orders::submit` whose `onObserverDeliver` passed the call to `next` and returned `{ order_id, status }` instead of the handler's output: `result` is the hook's value and there is no `observed`. The matches go in `invocationsobserved-routed.json`. | dec. 36; v2 §4.2, §8.2 | as above |

## Engine → worker

| File | Message | What it pins | From | Optional fields |
|---|---|---|---|---|
| `workerregistered.json` | `workerregistered` | This engine's `features`: `observe.v1`, `observe.v2`, `local-dispatch.v1`, in that order. | dec. 2; v2 §8.1 | `reattach_token`, `features` |
| `workerregistered-v1.json` | `workerregistered` | What a 0.25 engine sends: `observe.v1` and `local-dispatch.v1`, no `observe.v2`. A v2 SDK then leaves each function that declares `args_format`, `private` or `observable: false`, or sets `onObserverDeliver`, unregistered, logs `UNSUPPORTED` once for it, and runs everything else. It delivers local calls only, sends no `observed`, and sends `output` where it would set `output_is_input`. | dec. 4, 18; v2 §11 | as above |
| `workerregistered-legacy.json` | `workerregistered` | What a pre-0.25 engine sends: no `features`, so `observe()` fails with `UNSUPPORTED` and nothing runs locally. | 0.25 §1 | as above |
| `registrationrejected-unsupported.json` | `registrationrejected` | The engine's answer to `registerfunction-v1-owner.json`: `code: UNSUPPORTED`, the function and its namespace, and `owner_worker_id: ""`, as on every rejection that is not a conflict. Only that registration is refused; the connection stays open. | dec. 1, 37; v2 §11 | `worker_name`, `function_id` |
| `registrationrejected-invalid-args-format.json` | `registrationrejected` | The answer to a `registerfunction` whose `args_format` is not a valid JSON Schema: `code: INVALID_ARGS_FORMAT`. | dec. 11 | as above |
| `invocationresult.json` | `invocationresult` | The engine's answer to `invokefunction-observer-call.json`: the target's result under the frame's `invocation_id` and `function_id`, without `observed`. When the target's host listed `observed`, the engine activates those ids and strips the field before forwarding. | dec. 13, 36; v2 §4.6 | `result`, `observed`, `error`, `traceparent`, `baggage` |
| `invocationresult-not-function-owner.json` | `invocationresult` | The engine's refusal of `invokefunction-observer-call-namespace.json` from a connection that does not host the observer's function: `error.code: NOT_FUNCTION_OWNER` under the frame's `invocation_id` and `function_id`, no `result`, no `observed`. Every refusal of a request/response call through an observer has this shape. | v2 §4.6, §8.5; dec. 13 | as above |
| `observeresult.json` | `observeresult` | The reply to `observe` (also to a replay): the observer's current state. | 0.25 §2.3 | `error`, `warnings` |
| `observeresult-error.json` | `observeresult` | A rejected observer: `state: error`, the `error`, and `warnings` from filter paths that could not be verified. | 0.25 §2.3 | as above |
| `observerstatechanged.json` | `observerstatechanged` | A lifecycle transition (here to `active`). `fires` is always sent. | 0.25 §2.3, §3.3 | `reason` |
| `observerstatechanged-reason.json` | `observerstatechanged` | A transition with its `reason`: the `HTTP_ROUTE_CONFLICT` that `http::on-request`'s `onObserverAdded` threw for a route `GET /users/:uid` next to `GET /users/:id`. | v2 §10.3; 0.25 §2.3 | as above |
| `functionregistered.json` | `functionregistered` | The `registerfunction` ack with the full current observer set; `local_dispatch: true`. `revision` is per `(namespace, function)` and goes up by one with every announcement: this message, each `observeradded` and each `observerremoved` (so an ack for an earlier announcement is stale). Always accepted; it replaces the set the SDK holds, also for a standby that takes over the function. | v2 §7, §8.4; 0.25 §2.3 | none |
| `functionregistered-empty.json` | `functionregistered` | No observers yet, and `local_dispatch: false` (e.g. the session has RBAC middleware). `observers` is sent even when empty. | v2 §8.4; 0.25 §2.3 | none |
| `observeradded.json` | `observeradded` | One observer attached to a function this connection hosts, as the host sees it: `id`, `args` (`{}` when none), the normalized `filters`, the `target` function id (its namespace is not disclosed), `capture` and `on`, always sent resolved; never `deliver`, metadata, policy or the observing worker. | v2 §4.7, §8.4; 0.25 §2.3 | none |
| `observerremoved.json` | `observerremoved` | One observer detached. An SDK ignores an added/removed message whose `revision` is not greater than the one it holds. | v2 §8.4; 0.25 §2.3 | none |
| `invokefunction-activation.json` | `invokefunction` | An activation: `data` is the `Observed` envelope (per the observer's `capture`; the observed `input` is the payload as its caller sent it, without the engine's `_caller_worker_id` stamp), `metadata` is the observer's sidecar, `caller` is the engine (`worker_name: "iii-engine"`, the observed function and invocation), `observe_depth` is the observed depth + 1, `activation` carries `observer_id` and `activation_id`. It reaches the target with an `invocation_id` and no `action`; the target's SDK answers with an `invocationresult`. The engine discards that result for the observed side, but activates the ids in its `observed` (the target's own observers, when its host declared `observe.v2`), as for any routed answer. The answer frees the observer's `max_in_flight` slot (so does `observers.activation_timeout_ms`, 30 s by default, for an activation never answered). | v2 §4.4; dec. 36; 0.25 §3.4 | `caller`, `observe_depth`, `activation` |
| `invokefunction-activation-engine-source.json` | `invokefunction` | An activation from an engine-hosted source: `engine::on-observers-changed`, called once per observer state change with the event `{ observer_id, function_id, target, state, reason? }` (here the `HTTP_ROUTE_CONFLICT` of `observerstatechanged-reason.json`). The source's handler returns its event, so the envelope carries it as both `input` and `output`. The engine makes the call, so the envelope has no `caller`. | dec. 6, 7; 0.25 §3.7 | as above |
| `invokefunction-caller.json` | `invokefunction` | A routed call forwarded by the engine: `caller.worker_id` / `worker_name` stamped from the calling connection, `function_id` / `invocation_id` / `parent` asserted by the calling SDK, `observe_depth` copied from the caller. To an `observe.v2` host it always carries an `invocation_id`. | v2 §8.2; 0.25 §2.4 | as above |

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
- `invocationsobserved` items are checked one by one. An item that does not
  decode, or whose `status` is neither `success` nor `error`, is dropped alone
  (counted as `INVALID_REPORT` in `iii.observer.dropped.total`) and the rest of
  the batch is applied. The engine stamps `caller.worker_id` and
  `caller.worker_name` of each item from the reporting connection, except on
  an item for a call it routed to that connection (dec. 36), which keeps the
  `worker_id` and `worker_name` the engine forwarded the call with.
- **`matched` and `observed` (v2 §8.2, dec. 4, 20, 36).** Both list the
  observers `next` was given whose `on` and filters matched the call; when a
  deliver hook calls `next` more than once, the union, each id once. A report
  item that would carry both `input` and `output` of a handler that returned
  its input carries `input` and `"output_is_input": true` instead of
  `output`, and the engine fills `output` from `input`. Node, browser and
  Python detect it by identity, Rust and Go by comparing the serialized
  bytes. An `invocationresult` has no such flag: the engine takes `output`
  from its `result`, which is why a host lists `observed` only when that
  `result` is the handler's output.
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
  private with `observeresult` error `FORBIDDEN`.
- **Removed trigger frames (dec. 5).** The engine drops each of the five 0.25
  trigger frames with one `warn` per connection and message type. No v2 SDK
  sends them.
- Unknown message types and unknown fields must be ignored, as today.
