# observe v2 — acceptance checklist

Every behavior agreed for observe v2, with how it is verified. The work is done
when every item holds in the engine, all five SDKs (Rust, Node, browser,
Python, Go), the CLI, every worker in `iii-hq/workers`, and the docs. Sources:
[`README.md`](README.md) (spec) and [`decisions.md`](decisions.md).

Verification kinds: **test** (an automated test asserts it), **grep** (a
search over the tree returns nothing), **bench** (a benchmark or load test
measures it), **read** (a reviewer confirms it in the code or docs).

## A. The model

| # | Requirement | Verify |
|---|---|---|
| A1 | Every function is observable unless registered `observable: false` (`NOT_OBSERVABLE`). | test |
| A2 | One delivery path. No delivery modes: no `delivery`, `auto`/`owner`, `Observe::owner()/auto()`, `ObserveHandler`, `obs.emit`, `obs.call`, owner-only invocation rule. | grep, test |
| A3 | What observers observe is the handler's return value (`output`); `response_format` describes it. | test |
| A4 | `args` belong to the function's worker: validated against `args_format`, handed to the hooks, never interpreted by iii. `args_format` is allowed on any function. | test |
| A5 | `filters` belong to iii and are evaluated wherever the call is delivered (host or engine). | test |
| A6 | `capture` stays: it picks which of `input`, `output`/`error`, `caller` the envelope carries; ids, status and timing are always present; `capture: []` moves no payloads. | test |
| A7 | `deliver: 'envelope' \| 'input' \| 'output'` picks what the target receives from iii's delivery. | test |
| A8 | `observe('fn::id', target)` shorthand in every SDK (Go: `ObserveFunction`). | test |

## B. The four hooks

| # | Requirement | Verify |
|---|---|---|
| B1 | Hooks are methods, never registration options: on the client (with a function id, or without one for every function the worker hosts) and on `FunctionRef` (id filled in). Rust/Go use `_for` / `For` for the id form. | read, test |
| B2 | `onObserverAdded` / `onObserverRemoved` defaults are iii's normal behavior (accept; forget). A hook that resolves accepts; a throw rejects with `ObserveRejection` code or `OBSERVE_REJECTED`. | test |
| B3 | Worker-wide and function `onObserverAdded` both run, worker first; removal runs function then worker. No `next` in these hooks. | test |
| B4 | A rejection unwinds: the `onObserverRemoved` of each level whose `onObserverAdded` resolved runs. | test |
| B5 | `onObserverRemoved` runs exactly once per accepted observer (unobserve, disposal, its observing worker leaving, `obs.reject()` connected or not), never for a rejected one; its errors are logged. | test |
| B6 | Hooks of one function run one at a time; a hook past 30 s fails with `OWNER_TIMEOUT`. The engine counts the 30 s from when the host moved on to the observer: its announcement, or the host's answer (or timeout) for the observer announced before it. | test |
| B7 | A hook set by id before registration (every SDK), or chained on `registerFunction` (Node and browser), sees every observer; in Node and the browser the SDK acks no observer before the registration call returned. A hook set later is caught up and logs one warning. | test |
| B8 | Hooks stored by id survive `unregister()` and re-registration; a hook id never registered logs a warning after the connection registers. | test |
| B9 | `null` / `None` / `clear_*` removes a hook. | test |
| B10 | `onObserverMatch` exists per function, as a predicate `(obs, call) => boolean` or keys `{ observer, call }`. iii delivers to matched observers with filters, `on`, policy and `deliver` applied. Default: every observer matches. | test |
| B11 | Keys: computed once per accepted observer and once per call; a call matches observers under any of its keys, each once; wildcards by listing; the index rebuilds when the hook is set while observers are active; a throwing predicate is no match (logged once); a throwing observer key function rejects the observer. | test |
| B12 | `isObserved()` is one atomic load; `isObserved(call)` on a key-matched function looks up the call's keys. | test, bench |
| B13 | `onObserverDeliver` exists per function, takes `(call, observers)` (the matched observers, a snapshot from call start), has no `next`, runs in the function's invocation context and span, and its return/throw is what the caller gets. iii delivers nothing for that function. | test |
| B14 | No hook takes a `next` parameter or calls `next`, and no `keyed(`, `Keyed`, `as_keyed` or `defaultDeliver` identifier exists anywhere. The grep matches identifiers and hook signatures (`\bkeyed\(\|\bKeyed\b\|\bas_keyed\b\|\bdefaultDeliver\b`, `(onObserver\|on_observer_\|OnObserver)\w*\(.*\bnext\b`), not the English words "next" and "keyed" in prose. | grep |

## C. Calls

| # | Requirement | Verify |
|---|---|---|
| C1 | `call` (Python `call`/`call_async`, Go `Call`) with `CallRequest`, `CallAction` is the API in every SDK. | test |
| C2 | `trigger`, `trigger_async`, `Trigger`, `TriggerRequest`, `TriggerAction` exist only as aliases of the call API, deprecated in types/docs (JSDoc, `#[deprecated]`, `// Deprecated:`, docstring), no runtime warning. | test, read |
| C3 | Call through an observer: `call({ observer, payload })`; frame carries `function_id = obs.target` and `activation.observer_id`; engine routes to the stored target, applies policy, metadata, loop guard; only the host of the observed function may (`NOT_FUNCTION_OWNER`); `OVERFLOW` over `max_in_flight`; refused void calls dropped and counted. | test |
| C4 | `Observer.accepted()` resolves after the ack; a call through the observer after it reaches an active observer. A host is announced no observer whose observing worker is away; it gets the observer again when that worker reattaches (decision 47). | test |
| C5 | CLI: `iii call <fn>` is the subcommand; `trigger` and `t` are aliases; telemetry records `call`. | test |

## D. Function options

| # | Requirement | Verify |
|---|---|---|
| D1 | `private: true`: only the hosting connection may invoke (others and the engine get `FORBIDDEN`); observable; cannot be a target. | test |
| D2 | A worker's own local call to its private function passes input and result by reference (no copy); Node freezes them when `NODE_ENV === 'development'`. | test, bench |
| D3 | `observable: false` → `NOT_OBSERVABLE`. | test |
| D4 | `standby`: on `FUNCTION_NAMESPACE_CONFLICT` retry every 1 s ±20 %; takes over observers when the host leaves. | test |
| D5 | Invalid `args_format` → `INVALID_ARGS_FORMAT`. | test |

## E. Wire and engine

| # | Requirement | Verify |
|---|---|---|
| E1 | Features `observe.v1`, `observe.v2`, `local-dispatch.v1` advertised; v2 SDKs declare v1 and v2. | test |
| E2 | The host delivers its own calls, local (batched report) and routed (`invocationresult.observed`, stripped before forwarding, ids checked). The engine evaluates only for hosts without `observe.v2`, engine-hosted sources and HTTP-invoked functions. | test |
| E3 | A routed call to an unobserved function costs one load and no copy; 1M unobserved local calls send zero frames. | test, bench |
| E4 | `output_is_input` on report items when the handler returned its input, only to `observe.v2` engines; an event source's event is serialized once. | test, bench |
| E5 | No host-reported items for routed calls (`INVALID_REPORT`). | test |
| E6 | `_caller_worker_id` stripped from every observed input and output; a call through an observer carries it as every call a worker makes does (decision 49). | test |
| E7 | v2-only functions (`args_format`, `private`, `observable: false`, `onObserverMatch`/`onObserverDeliver`) are held on engines without `observe.v2` with one `UNSUPPORTED` log, re-decided per connection. | test |
| E8 | 0.25 `observe` object decoded per decision 37 (`delivery: owner` → `UNSUPPORTED`). | test |
| E9 | Builtins: trigger builtins removed; function rows carry `observer_count`, `args_format`, `private`, `observable`; `engine::workers::info` carries `functions`, `observers`, and a builtin worker's `functions` include the engine-hosted sources it hosts; `engine::on-observers-changed` exists. | test |
| E10 | Engine-hosted sources are private, return the event, caller `{ worker_name: "iii-engine" }`; stream sources `stream::on-changed/on-join/on-leave`. | test |
| E11 | Every fixture in `sdk/fixtures/protocol/observe` round-trips in the engine and all five SDKs; no fixture or rule about next, per-next pooling, unchanged calls, `LATE_DELIVERY` or routed items remains. | test, grep |
| E12 | Metrics labels `engine` / `host` / `observer_call`; telemetry heartbeat observer counts. | test |
| E13 | RBAC: trigger keys removed (ignored with one warn); registration hook sees and may rewrite `args_format`, `private`, `observable`. | test |

## F. Triggers removed everywhere

| # | Requirement | Verify |
|---|---|---|
| F1 | Engine: no trigger registry, trigger messages (removed frames warn once per connection and type), trigger builtins, trigger RBAC hooks, legacy shim, `legacy_trigger_type`, queue compat module, owner delivery. | grep, test |
| F2 | SDKs: no `registerTrigger`, `registerTriggerType`, `unregisterTrigger(Type)`, `TriggerHandler`, typed trigger configs, `EngineTriggers` (→ `EngineSources`), `Observer.legacy`. | grep |
| F3 | Workers: every trigger type is an observable function per `iii-hq/workers/tech-specs/2026-10-observe-v2/functions.md`; every binding is `observe`; every `trigger()` is `call()`; UI, READMEs, SKILL.md, manifests and CI say functions and observers. | grep, test |
| F4 | The only remaining "trigger" words are: the call aliases (`trigger`, `trigger_async`, `Trigger`, `TriggerRequest`, `TriggerAction`, `iii trigger` / `t`); database triggers; CI event names; "trigger" as an English verb; the 0.25 names the engine and SDKs still decode, warn about or refuse (the removed frames, RBAC keys and builtins, `legacy_trigger_type`), with their tests and fixtures; the upgrade guides (`docs/*/upgrading/`, the SDK READMEs' "Migrating from 0.25" sections, the `iii-observe` skill's "Code written for 0.25"); the configuration keys stored by earlier releases that a worker still reads to load them (state's `triggers_enabled`, `config::EARLIER_EVENTS_KEY`); the changelog; the redirects (`docs/docs.json`, the table in `docs/RELEASING.md`); the versioned and archived docs (`docs/` outside `docs/next`); dated historical specs and the superseded `tech-specs/engine-register-trigger-metadata.md`. | grep |

## G. Workers

| # | Requirement | Verify |
|---|---|---|
| G1 | Every worker builds and passes its own test suite against the v2 engine and SDKs. | test |
| G2 | database, cron, http, queue, state match spec §10 (match hooks, deliver hooks, changeFeed setup, `isObserved(call)` gates). | read, test |
| G3 | Harness: `harness::observe` / `harness::unobserve` agent tools; stored bindings translated at boot or retired with a notice; `harness::on-*` private sources; `observer_fired` records (readers accept `trigger_fired`); envelope deliveries; `expires_in_ms` backstop. | test |
| G4 | ADE: Observers screen (`ext:observers`, migrated state), `console::ui::asset` with `{ kind, path }`, initial sync through `accepted()`, injectable-UI `functionCalls` / `function-call` with old role accepted. | test, read |
| G5 | rbac-proxy: observe.v1/v2 forwarded, `local-dispatch.v1` only without middleware or for a function the session hosts privately (decision 46), removed frames dropped with warn-once, observer discovery filtered by access. | test |
| G6 | SDK pins `=0.26.0` / `0.26.0` / `>=0.26.0` / `v0.26.0`; no committed path overrides. | grep |

## H. Docs

| # | Requirement | Verify |
|---|---|---|
| H1 | Consumer pages lead with `iii.observe('a', 'b')` and `call`; provider hooks read as overriding iii defaults and stay off the consumer path. | read |
| H2 | Docs show the four hooks (`onObserverMatch`, no `next`, no `keyed`), `capture`, and every API name exactly as shipped, in every language tab. | read, grep |
| H3 | No corrective phrasing; Vale clean; `.skill.md` artifacts rendered; generated SDK and CLI references regenerated from the final code. | test |
| H4 | Upgrade guide `from-0-25-x` covers every row of spec §2 and §11. | read |

## I. Performance

| # | Requirement | Verify |
|---|---|---|
| I1 | Every row of spec §13 measured and within budget (Rust host; Node baseline recorded). | bench |
| I2 | Engine benches (`observe_dispatch/default/*`, `observe_fanout`, routed-observed) and loadtest scenarios (`source`, `match-keys`) run and are reported against the 0.25 baseline. | bench |
