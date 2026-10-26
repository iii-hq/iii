# observe wire fixtures

Golden JSON for every message and field that observe adds to the engine
protocol in 0.25 (`tech-specs/2026-10-13-observe/protocol.md` §2).

**The fixtures are the wire contract. SDKs must decode and encode these
exactly.** Every SDK's test suite decodes each file into its message type and
encodes it back to the same JSON value (key order and whitespace do not
matter; keys, values and the absence of keys do). The engine runs the same
round trip through its `Message` enum
(`engine/tests/protocol_observe_fixtures.rs`), so a file that is here is a
shape the engine both accepts and produces. If a fixture looks inconsistent
with the spec, report it; do not fork the format in one SDK.

## Rules every file follows

- **Tagging.** One JSON object per file, tagged by `type` (lowercase, the
  existing `#[serde(tag = "type", rename_all = "lowercase")]` scheme). The
  file name starts with the tag; anything after the first `-` names the
  variant.
- **Optional means absent.** An optional field that is not set is left out,
  never sent as `null`. Decoders must still accept `null` for it.
- **`null` is a value where it carries meaning.** In `invocationsobserved`
  items, `input`, `output`, `error` and `metadata` keep `null` apart from
  absent: `"output": null` is a function that returned `null`, while a
  missing `output` was not captured or does not exist.
- **Enums are lowercase strings.** `delivery`: `auto | owner`.
  `capture[]`: `input | output | error | caller | metadata`.
  `on`: `success | error | complete`. `deliver`: `envelope | input | output`.
  `state`: `pending | active | suspended | error | disposed`.
  `status`: `success | error`. Decoders must accept a value they do not know
  (a newer peer) without failing the frame, and keep it as an unknown value.
- **Defaults (CONTRACT D1).** Absent `on` is `success`, absent `capture` is
  `["input", "output", "caller"]`, absent `deliver` is `envelope`, absent
  `observe.delivery` is `auto`, absent `observe.allow` is `true`, absent
  `observe_depth` is `0` (and `0` is omitted when sending). `capture: []` is
  valid: the envelope then carries ids, status and timings only (D14).
- **Unknown values are refused, never defaulted (D14).** The engine answers an
  `observe` with an unknown `on` / `deliver` / `capture[]` value with
  `observeresult` error `UNSUPPORTED`, and a `registerfunction` with an unknown
  `observe.delivery` with `registrationrejected` `INVALID_OBSERVE_DESCRIPTOR`.
- **Malformed new fields are not fatal.** The engine reads `caller`,
  `observe_depth` and `activation` on `invokefunction` leniently (a bad value is
  treated as absent), answers an `observe` it cannot decode but whose `id` is
  a string with `observeresult` error `INVALID_ARGS`, and an `invokefunction`
  it cannot decode but whose `invocation_id` is a UUID with an
  `invocationresult` error.
- **Ids are strings.** Observer ids are client-generated UUID v4 strings,
  stable across replays (CONTRACT D3). `invocation_id`, `activation_id` and
  the ids inside `caller` are strings too.
- **Codes are exact strings** from protocol.md §4 (CONTRACT D4): error codes
  in `error.code`, state reasons in `reason.code`, drop reasons in
  `dropped_reports[].reason` (`BUFFER_FULL`, `TOO_LARGE`).
- **Numbers.** `revision`, `fires`, `count`, `errors`, `window_ms`,
  `started_at_ms`, `max_fires`, `expires_in_ms`, `max_in_flight` and
  `observe_depth` are non-negative integers. `duration_ms` and
  `duration_ms_sum` are floats (the fixtures use non-integral values so the
  round trip is unambiguous in every language).
- **Feature gate (CONTRACT D2, D9, D17).** The engine pushes the engine →
  worker messages below only to connections that declared `observe.v1` in
  `engine::workers::register` (the replies to a connection's own `observe` go
  to it regardless), and adds `caller`, `observe_depth` and `activation` to an
  `invokefunction` only for such connections. It decodes every worker → engine
  message from anyone. A connection's features are fixed by the first
  `engine::workers::register` that lists any (at most 32 names of at most 64
  bytes); later declarations are ignored.

## Worker → engine

| File | Message | What it shows | Optional fields |
|---|---|---|---|
| `observe.json` | `observe` | Every field set: a cron observer with args, filters, facet, policy and metadata. | `source.namespace`, `source.args`, `source.filters`, `source.capture`, `source.on`, `source.deliver`, `target.namespace`, `metadata`, `max_fires`, `expires_in_ms`, `max_in_flight` |
| `observe-minimal.json` | `observe` | Only the required fields: `id`, `source.function_id`, `target.function_id`. Everything else takes its default. | as above |
| `observe-auto.json` | `observe` | An auto observer on a plain function: `on: error`, a filter on `input`, a custom `capture`, a policy. No args. | as above |
| `unobserve.json` | `unobserve` | Dispose an observer this connection owns. | none |
| `observerack.json` | `observerack` | The owner accepts an added observer, echoing `function_id`, `namespace`, `id` and `revision` from `observeradded`. | `namespace` (absent: the sending connection's namespace), `error` |
| `observerack-error.json` | `observerack` | The owner rejects it; `error.code` is the owner's code (here `INVALID_CRON_EXPRESSION`, or `OBSERVE_REJECTED` / `OWNER_TIMEOUT`). Also the shape of a late rejection. | as above |
| `invocationsobserved.json` | `invocationsobserved` | One batch with three items: an auto local call that failed (`status: error`, `error`), an auto local call that returned `null` (`"output": null`), and an owner `obs.emit` event (database.md §2.2's row-changed event) coalesced for two observers, whose caller has a `parent`. | per item: `namespace`, `input`, `output`, `error`, `caller`, `metadata`, `traceparent`, `baggage` |
| `localinvocationstats.json` | `localinvocationstats` | The periodic counts of local invocations and SDK-side report drops; sent only when non-zero. | `functions[].errors`, `functions[].duration_ms_sum` (default 0); both arrays default to empty |
| `registerfunction.json` | `registerfunction` | A function without an `observe` descriptor: observable, `delivery: auto`, no args. | `description`, `request_format`, `response_format`, `metadata`, `invocation`, `observe` |
| `registerfunction-owner.json` | `registerfunction` | An owner-delivered function: `delivery: owner`, `args_format`, `legacy_trigger_type`, `allow`. Callbacks never travel. | `observe.*` (all four) |
| `registerfunction-not-observable.json` | `registerfunction` | `allow: false`: every `observe` on it fails with `NOT_OBSERVABLE`. | `observe.*` |
| `invokefunction-owner-call.json` | `invokefunction` | An owner delivery (`obs.call`), request/response: `activation.observer_id` only, the `caller` the SDK asserts (the handler it runs in), `observe_depth` omitted because it is 0, no `namespace` (D13). | `caller`, `observe_depth`, `activation` (and every pre-0.25 optional field) |
| `invokefunction-owner-call-namespace.json` | `invokefunction` | An owner call that carries a `namespace` anyway. SDKs do not send one (D13); the engine routes an `activation` call to the observer's stored target and ignores it. | as above |
| `invokefunction-legacy.json` | `invokefunction` | A call from a pre-0.25 SDK, without `caller`, `observe_depth` or `activation`. It must keep decoding. | as above |
| `invokefunction-workers-register.json` | `invokefunction` | The fire-and-forget `engine::workers::register` call declaring `features` in its payload (no `invocation_id` on a `void` call). | `data.features` (absent: a pre-0.25 SDK, no features) |
| `unregistertriggertype.json` | `unregistertriggertype` | A provider retiring a trigger type. The engine removes it only when the sending connection is its provider. | `namespace` (absent: the sending connection's namespace) |

## Engine → worker

| File | Message | What it shows | Optional fields |
|---|---|---|---|
| `workerregistered.json` | `workerregistered` | The engine's `features`: `["observe.v1", "local-dispatch.v1"]`. An SDK enables `observe()` only when `observe.v1` is listed. | `reattach_token`, `features` |
| `workerregistered-legacy.json` | `workerregistered` | What a pre-0.25 engine sends: no `features`, so `observe()` fails with `UNSUPPORTED` and nothing runs locally. | as above |
| `observeresult.json` | `observeresult` | The reply to `observe` (also to a replay): the observer's current state. | `error`, `warnings` |
| `observeresult-error.json` | `observeresult` | A rejected observer: `state: error`, the `error`, and `warnings` from filter paths that could not be verified. | as above |
| `observerstatechanged.json` | `observerstatechanged` | A lifecycle transition (here to `active`). `fires` is always sent. | `reason` |
| `observerstatechanged-reason.json` | `observerstatechanged` | A transition with its `reason` (an owner rejection, `HTTP_ROUTE_CONFLICT`). | as above |
| `functionregistered.json` | `functionregistered` | The `registerfunction` ack with the full current observer set; `local_dispatch: true`. `revision` is per `(namespace, function)` and goes up by one with every announcement: this message, each `observeradded` and each `observerremoved` (so an ack for an earlier announcement is stale). Always accepted; it replaces the set the SDK holds. | none (`observers[].legacy` is optional) |
| `functionregistered-empty.json` | `functionregistered` | No observers yet, and `local_dispatch: false` (e.g. the session has RBAC middleware). `observers` is sent even when empty. | as above |
| `observeradded.json` | `observeradded` | One observer attached to a function this connection hosts. The owner sees `id`, `args` (`{}` when none), the normalized `filters`, the `target` function id (its namespace is not disclosed, CONTRACT D6), `capture` and `on` (D15); never `deliver`, metadata, policy or the observing worker. The engine always sends `capture` and `on` resolved. | `observer.legacy` |
| `observeradded-legacy.json` | `observeradded` | An observer the legacy shim created from an old `registertrigger`: `legacy.trigger_type` and `legacy.config` (the old trigger config, read by the owner as args), `args: {}`, `filters: {}`. | as above |
| `observerremoved.json` | `observerremoved` | One observer detached. An SDK ignores an added/removed message whose `revision` is not greater than the one it holds. | none |
| `invokefunction-activation.json` | `invokefunction` | An auto activation: `data` is the `Observed` envelope (per the observer's `capture`; the observed `input` is the payload as its caller sent it, without the engine's `_caller_worker_id` stamp), `metadata` is the observer's sidecar, `caller` is the engine (`worker_name: "iii-engine"`, the observed function and invocation), `observe_depth` is the observed depth + 1, `activation` carries `observer_id` and `activation_id`. Fire-and-forget for the observed side (D16), it reaches the target exactly as the engine forwards any call: with an `invocation_id` and no `action`. The target's SDK answers it with an `invocationresult` as for any call; the engine discards the result, and the answer frees the observer's `max_in_flight` slot (so does `observers.activation_timeout_ms`, 30 s by default, for an activation never answered). | `caller`, `observe_depth`, `activation` |
| `invokefunction-caller.json` | `invokefunction` | A routed call forwarded by the engine: `caller.worker_id` / `worker_name` stamped from the calling connection, `function_id` / `invocation_id` / `parent` asserted by the calling SDK, `observe_depth` copied from the caller. | as above |

## Notes for SDK authors

- `caller.parent` is one level only: a `parent` inside `parent` is not part of
  the format, and the engine drops it.
- The `filters` inside `functionregistered` / `observeradded` are the
  observer's filters in normal form (filters.md §5): `{}` matches every event,
  anything else is `{"$and": [...]}` at every level. Compile them as given.
- An `observe` without `filters` (or with `filters: null`) matches every event.
- An owner acks every observer it is told about, each with the `revision`
  of the message that told it: every entry of `functionregistered.observers`
  (also the ones it already held before a re-registration: a new
  registration starts a new owner relationship) and every `observeradded`.
  Until then the observer stays `pending`; after
  `observers.owner_ack_timeout_ms` (30 s by default) it fails with
  `OWNER_TIMEOUT`. An ack whose `revision` is older than the announcement is
  ignored.
- `invocationsobserved` items are checked one by one. An item that does not
  decode, or whose `status` is neither `success` nor `error`, is dropped alone
  (counted as `INVALID_REPORT` in `iii.observer.dropped.total`) and the rest of
  the batch is applied. The engine stamps `caller.worker_id` and
  `caller.worker_name` of each item from the reporting connection.
- An owner delivery (`obs.call`) fails with the codes of protocol.md §3.5, or
  with `TIMEOUT` when the target does not answer within
  `observers.activation_timeout_ms`.
- An observer with `legacy` (`observeradded-legacy.json`) was made by the
  engine's legacy shim from an old `registertrigger` whose type no live
  provider serves (protocol.md §6). Validate `legacy.config` as the old trigger
  handler did, read it as the observer's args, and ack or reject it like any
  observer; the old registrant gets one `triggerregistrationresult` with the
  outcome (your rejection's `error` included). Only an owner that declared
  `observe.v1` stands in for a trigger type: `legacy_trigger_type` on any other
  owner is ignored.
- Unknown message types and unknown fields must be ignored, as today.
