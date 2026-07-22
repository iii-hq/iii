# The injection protocol — `console:*` triggers

How UI assets travel: the three trigger types the console worker owns (two
asset types plus the tab-subscription type), the config contract, where the
bytes come from, override semantics, lifecycle, and trust.

## The three trigger types

The console worker registers three trigger types at boot, **before**
`functions::register_all` (the approval-gate/memory ordering convention,
`workers/approval-gate/src/events.rs:195-227`). Two carry assets and are
registered by workers; the third is the live-update subscription, registered
by console tabs. Type ids are namespaced `console:*` — the single-colon
prefix marks ownership in every discovery listing (the engine treats type ids
as opaque strings), while the short values `script`/`style` live on as the
**asset kind** in push payloads and the manifest.

| Type id | Registered by | Carries | Applied by the SPA as |
|---|---|---|---|
| `console:script` | workers | an ESM JavaScript asset, served `text/javascript` | `import()` + `setup(host)` ([slots-and-api.md](slots-and-api.md)) |
| `console:style` | workers | a CSS asset, served `text/css` | `<link rel="stylesheet">` swap ([hot-reload.md § CSS](hot-reload.md#css-style-triggers)) |
| `console:assets` | console tabs (browser SDK) | a subscription — its `function_id` is the tab handler the console pushes `sync`/`set`/`delete` events to ([§ The subscription type](#the-subscription-type-consoleassets)) | n/a — it feeds the loader |

Registration uses the pinned Rust SDK (`iii-sdk = "=0.21.6"`,
`workers/console/Cargo.toml:17`) surface that already exists:

```rust
// iii/sdk/packages/rust/iii/src/iii.rs:1098 — register_trigger_type
// iii/sdk/packages/rust/iii/src/triggers.rs:10-30 — what the handler receives
pub struct TriggerConfig {
    pub id: String,             // engine trigger id (uuid, minted by the registrant's SDK)
    pub function_id: String,    // the CONTENT FUNCTION (see below)
    pub config: Value,          // { "path": "state/page.js" }
    pub metadata: Option<Value>,
}

#[async_trait]
pub trait TriggerHandler: Send + Sync {
    async fn register_trigger(&self, config: TriggerConfig) -> Result<(), Error>;
    async fn unregister_trigger(&self, config: TriggerConfig) -> Result<(), Error>;
}
```

Both types publish a config schema via
`.trigger_request_format::<ScriptTriggerConfig>()`
(`iii/sdk/packages/rust/iii/src/iii.rs:103-155`). The engine stores that schema
and surfaces it as `configuration_schema` in `engine::triggers::info`
(`iii/engine/src/workers/engine_fn/mod.rs:805`) but **never validates config
against it** — validation is the console handler's job (below).

The engine wire messages involved are existing protocol, unchanged
(`iii/engine/src/protocol.rs:40-70`):

```rust
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Message {
    RegisterTriggerType { id, description, trigger_request_format, call_request_format },
    RegisterTrigger { id, trigger_type, function_id, config, metadata },
    TriggerRegistrationResult { id, trigger_type, function_id, error },
    UnregisterTrigger { id, trigger_type: Option<String> },  // #[serde(default)] — may be absent on the wire
    // ...
}
```

(On the engine→type-owner leg, `trigger_type` is always filled with
`Some(..)` — `iii/engine/src/worker_connections/traits.rs:135-138` — so the
console's `TriggerHandler` always receives the type; registrant-originated
frames may legitimately omit it.)

## The config contract

```jsonc
// trigger_request_format for both ASSET types (draft-07, published at type registration)
{
  "type": "object",
  "required": ["path"],
  "additionalProperties": false,
  "properties": {
    "path": {
      "type": "string",
      "description": "Asset identity. Convention: <worker>/<name>.<ext>. Re-registering a path overrides it."
    }
  }
}
```

`console:assets` publishes the trivial schema — an empty object
(`"additionalProperties": false`, no required keys). Filtering (by kind or
path prefix) is deliberately absent from v1: every subscriber gets every
event.

`path` rules, enforced by the console's `TriggerHandler` (registrations that
violate them are **rejected**, see [Acks](#acks-and-rejection)):

- lowercase kebab/dot segments: `^[a-z0-9][a-z0-9._-]*(/[a-z0-9][a-z0-9._-]*)*$`
- no `.` / `..` segments, no leading slash, no backslashes (it becomes a URL
  path segment under `/ui/`)
- extension must match the type: `.js` for `console:script`, `.css` for
  `console:style`
- convention (not enforced): first segment is the registering worker's name —
  `state/page.js`, `state/theme.css` — because the engine does **not** tell the
  type owner who registered (the forwarded `RegisterTrigger` carries no worker
  identity, `iii/engine/src/worker_connections/traits.rs:57-64`), so the path
  prefix is the human-readable attribution.

`metadata` is passed through and ignored by v1 (reserved). Note the browser
SDK cannot send trigger metadata at all
(`RegisterTriggerMessage` has no such field,
`iii/sdk/packages/node/iii-browser/src/iii-types.ts:42-53`) — one more reason
everything semantic lives in `config`.

## Where the bytes come from: the content function

A `console:script`/`console:style` trigger never *fires* in the classic
sense. Its mandatory
`function_id` names the **content function** — a normal iii function on the
registering worker that the console invokes to fetch source:

| | Contract |
|---|---|
| Function id | worker's choice; convention `<worker>::ui-content` (one function serves all of that worker's assets) |
| Input | `{ "path": string }` — the path from the trigger config |
| Output | `{ "content": string, "content_type"?: string }` — `content_type` defaults from the path extension |
| Errors | any bus error fails the fetch (see [Fetch policy](#fetch-policy)) |

The console fetches with the same bounded-retry pattern it already applies to
the configuration worker (`trigger_with_retry`,
`workers/console/src/configuration.rs:18-21`) — but at live-registration time
the budget is deliberately tighter than that precedent's 3×5s: the engine
awaits the owner's ack for only 10s and then **fails open** (see
[Acks](#acks-and-rejection)), so the whole handler run must fit inside that
window.

### Fetch policy

| Moment | Retry budget | On success | On failure |
|---|---|---|---|
| Live registration (worker present) | 2 attempts × 3s timeout + 250ms backoff (≈6.5s worst case, safely inside the 10s ack window) | hash, store, publish | **reject the registration** — the error reaches the registrant (see Acks) |
| Replay after console restart (`replay_trigger` is fire-and-forget, `iii/engine/src/trigger.rs:158-169` — no ack window to honor) | 3 attempts × 5s + backoff | same | drop with a `warn!`; the asset is simply absent from the manifest until the worker re-registers |

Fetched bytes are cached in the console worker's in-memory registry and served
from there — browsers never wait on a bus round-trip per request, and content
is pinned to the hash advertised for it.

Size cap: the console rejects content over **8 MiB** per asset. (For scale: the
engine imposes no explicit WS message cap of its own — plain axum 0.8 upgrades
throughout, `iii/engine/Cargo.toml:117` — but multi-MiB assets are a smell; see
[authoring.md](authoring.md) on bundling.)

### Style lint (warn-only)

After a successful `console:style` fetch the console runs a cheap static scan for
selectors that escape the asset's scope — rules not nested under a
`[data-iii-ui="…"]` prefix that target `:root`, `html`, `body`, `*`, or bare
element names, plus `@font-face` (inherently unscopable). Findings become a
`warnings: [string]` array on the asset's manifest entry
([hot-reload.md § The manifest](hot-reload.md#the-manifest-debug-surface)) and one
`warn!` log line naming the path. Warn-only in v1, never a rejection: the
scan cannot prove intent, and the trust model already admits hostile CSS
([Security](#security--trust-model)) — its job is catching the *accidental*
console-wide restyle (an unlayered injected rule beats the console's fully
layered CSS at equal specificity). Assets compiled through
`@iii-dev/console-build` never trip it
([authoring.md § Styling & Tailwind](authoring.md#styling--tailwind)).

## Override semantics ("same path ⇒ override")

Engine facts this is built on (all verified):

- Trigger identity is `id` **only**; two triggers with identical config coexist
  (`iii/engine/src/trigger.rs:187-198`).
- No SDK lets the caller pick the id — Node mints `crypto.randomUUID()`
  (`iii/sdk/packages/node/iii/src/iii.ts:265-267`), Rust mints `Uuid::new_v4()`
  (`iii/sdk/packages/rust/iii/src/iii.rs:1168-1197`), `engine::register_trigger`
  mints server-side (`iii/engine/src/workers/engine_fn/mod.rs:1601`).

So the console handler keeps a **two-key registry**:

```text
by_path: path → { trigger_id, kind, hash, content, content_type }
by_id:   trigger_id → path
```

**Serialization requirement (load-bearing).** The SDK delivers each incoming
`RegisterTrigger`/`UnregisterTrigger` callback on its **own spawned task**
(`tokio::spawn` in `handle_register_trigger`,
`iii/sdk/packages/rust/iii/src/iii.rs:2014-2093`) — invocations are concurrent
and completion order is not arrival order. Left unserialized, two rapid
re-registrations of one path (exactly what `esbuild --watch` produces) can
interleave: both observe the same old trigger id, both unregister it, and
whichever content fetch finishes *last* wins — possibly the older build,
leaving stale content live, plus a second un-superseded engine row. The
handler therefore drains **all** `console:*` register/unregister events —
assets *and* subscriptions, including the engine's unregister echo of step 4
below — through a single-consumer queue (mpsc) fed in WS-arrival order, with
the content fetch inside the serialized processing. Putting subscriptions on
the same queue is what makes a subscriber's initial `sync` and the
incremental pushes that follow mutually ordered. Arrival order is preserved, the echo lands
after the commit it echoes (hitting the unknown-id no-op branch), and holding
the section across step 4's `engine::unregister_trigger` call cannot deadlock
(the engine only awaits enqueueing the echo into the console's outbound
channel). The live-registration fetch budget above is per-event; queue
backlogs deep enough to threaten the ack window only occur during replay
bursts, where acks are fire-and-forget anyway.

**register_trigger(cfg)** (must be idempotent — replay re-delivers accepted
bindings):

1. Validate `cfg.config.path` (rules above); `Err` on violation.
2. Fetch content via `cfg.function_id`; `Err` on live-registration failure.
3. `hash = sha256(content)`. If `by_path[path]` exists with the **same
   trigger id and hash** → no-op (replay dedupe).
4. If `by_path[path]` exists with a **different trigger id** → supersede:
   remove the old id from `by_id`, log a `warn!` naming both, and call
   `engine::unregister_trigger { id: old_id }` (idempotent,
   `UnregisterTriggerResult { removed }`,
   `iii/engine/src/workers/engine_fn/mod.rs:420-461`). Last writer wins —
   including across *different* workers claiming the same path; the warn is
   the only arbitration.
5. If `by_id[cfg.id]` exists under a **different** `old_path` (id reuse — the
   Message path carries a registrant-chosen id verbatim, and the engine
   silently replaces its row for a duplicate id with no `UnregisterTrigger`,
   `iii/engine/src/trigger.rs:462-479`) → treat as an implicit move: remove
   `by_path[old_path]`, push `delete(old_path)` to subscribers, drop the
   stale `by_id` entry. Do **not** call `engine::unregister_trigger { cfg.id }` —
   the engine's row for `cfg.id` already *is* the new binding.
6. Update both maps, push `set` to every subscriber
   ([hot-reload.md](hot-reload.md)).
   The superseded id was already pruned from `by_id` in step 4, so its echo
   arrives as a true unknown-id no-op (and even a lingering stale entry could
   not cause a wrong delete — unregister step 2 below re-checks the current
   id).

**unregister_trigger(cfg)** — the engine sends only `{ id, trigger_type }`
(no config, `iii/engine/src/worker_connections/traits.rs:127-149`; the Rust SDK
fills `config: Value::Null`), hence `by_id`:

1. `path = by_id.remove(id)`; unknown id → no-op (this is exactly what a
   superseded trigger's own unregister — including the echo of step 4 above —
   looks like).
2. If `by_path[path].trigger_id == id` → remove the asset, push `delete` to
   subscribers, done. Otherwise no-op.
3. If `id` is a `console:assets` subscription → drop it from the subscriber
   set ([§ The subscription type](#the-subscription-type-consoleassets)).

Because step 4 prunes superseded rows from the engine registry, **at most one
live trigger per path exists** in steady state, so restart replay
(nondeterministic `DashMap` iteration order) needs no tiebreaker. One
qualifier: a registrant SDK whose local map still holds superseded ids replays
them all on its reconnect (see the lifecycle matrix), transiently re-running
the supersede path — churny but convergent, and avoided entirely by the
dev-loop discipline in [authoring.md](authoring.md#the-dev-loop-hot-reload-from-the-authors-chair).

## The subscription type: `console:assets`

The push half of hot reload, on the same primitive as everything else — no
stream worker, no extra endpoint. A console tab (browser SDK, through the
`/ws` proxy) registers:

```ts
client.registerTrigger({
  type: 'console:assets',
  function_id: `iii::console::ui-assets::${client.browserId}`,  // its own handler
  config: {},
})
```

Console handler behavior (same mpsc queue as the asset types):

1. **Record** `trigger_id → function_id` in the subscriber set.
2. **Immediately push `sync`** to that subscriber:
   `{ "event": "sync", "assets": [ { path, kind, hash }, … ] }` — the full
   current registry. Sync-on-subscribe is what makes ordering irrelevant:
   whatever committed before the subscription is in the sync, whatever
   commits after arrives incrementally. There is no seed/frame race to
   reason about — the earlier stream-worker draft of this design needed a
   subscribe-then-seed dance for exactly that gap.
3. On every later commit, push
   `{ "event": "set"|"delete", path, kind, hash }` to every recorded
   subscriber — one fire-and-forget `iii.trigger` per subscriber; a failed
   push is logged at debug and dropped (a dead tab's subscription is GC'd by
   the engine moments later anyway).
4. `UnregisterTrigger` for a subscription id → drop the subscriber. Tab
   disconnect does this implicitly — Message-path GC, exactly as for
   workers.

The handler ids stay `iii::`-prefixed on purpose:
`iii::console::ui-assets::<browserId>` is span-suppressed
(`is_iii_builtin_function_id`,
`iii/engine/src/workers/telemetry/mod.rs:202-220`) and the `/ws` proxy stamps
it `metadata.internal = true` like every browser registration
(`workers/console/src/proxy.rs:142-164`) — rebuild-loop pushes stay out of
the trace feed and the catalog, which the stream draft achieved with the
`iii:devtools:` stream-name prefix.

Consequences worth naming:

- **Console restart converges.** Replay re-delivers asset bindings and tab
  subscriptions in arbitrary `DashMap` order; a subscription processed early
  gets a partial `sync`, then the remaining asset commits as incremental
  pushes — every tab ends at the full registry, and the loader's hash dedupe
  makes the overlap free.
- **Parked while the console is down** like any other registration; the
  subscription activates (and gets its `sync`) when the console arrives.
- **Reconnect replays.** The browser SDK re-registers its triggers on
  reconnect; the console records the replayed registration as new and pushes
  a fresh `sync`.
- **Fan-out cost is explicit.** One bus invocation per subscriber per event —
  the same fan-out the stream worker performed internally, now visible in
  console code, with no backpressure beyond the outbound channel (accepted
  for v1; tabs are few).

## Acks and rejection

Rejection behaves differently per registration path — both are acceptable,
and both end with no stale engine state:

| Registrant path | Ack semantics (verified) | Effect of console `Err` |
|---|---|---|
| SDK `registerTrigger` (Message path — **the recommended path**) | fire-and-forget; a later error ack takes the engine's late-unwind: the trigger is removed from the registry and the `TriggerRegistrationResult` is forwarded to the registrant (spoof-guarded to the type owner, `iii/engine/src/engine/mod.rs:593-675`) | asset never existed; registrant's SDK logs the result |
| `engine::register_trigger` (function path) | engine awaits the owner's ack with a 10s timeout (`REGISTRATION_ACK_TIMEOUT`, `iii/engine/src/worker_connections/traits.rs:28`) and **fails open** on expiry: the trigger is inserted and the caller receives success + id | caller gets `trigger_registration_failed` synchronously — *provided the console acks within 10s* (the fetch budget above guarantees this in normal operation); past the window, a late console `Err` unwinds the trigger but the error is dropped — function-path triggers have no originator to notify (`iii/engine/src/engine/mod.rs:651-657`) |

The SDKs already produce these acks from the handler's `Result`
(`trigger_registration_failed` on `Err`,
`iii/sdk/packages/rust/iii/src/iii.rs:2014-2073`).

## Lifecycle matrix

| Event | Engine behavior (verified) | Console behavior | Tab behavior |
|---|---|---|---|
| Worker registers path | forward to owner | validate → fetch → publish | import + setup |
| Worker re-registers same path (hot reload) | forward (new trigger id) | supersede + prune old id | dispose → re-import |
| Worker calls `trigger.unregister()` | `UnregisterTrigger` to owner | remove asset, push `delete` | dispose |
| Tab registers `console:assets` | forward to owner | record subscriber, push `sync` | loader diffs the sync (hash dedupe) |
| Tab disconnects | its Message-path triggers GC'd | drop subscriber | — |
| Worker disconnects | all its Message-path triggers GC'd, one `UnregisterTrigger` each (`iii/engine/src/trigger.rs:221-253`, entry point `iii/engine/src/engine/mod.rs:1749-1758`) | same as unregister | dispose — injected UI dies with its worker |
| Worker reconnects | its SDK replays every registration still in its local map, original ids (Node: `iii/sdk/packages/node/iii/src/iii.ts:783-790`; Rust: `collect_registrations`, `iii/sdk/packages/rust/iii/src/iii.rs:1550-1566`, run on every reconnect) — **including superseded ids the worker never explicitly `unregister()`ed**, which re-run the supersede path one by one (churny but convergent: hash dedupe + unknown-id no-ops absorb it; the [authoring dev loop](authoring.md#the-dev-loop-hot-reload-from-the-authors-chair) keeps the map at one entry per path) | re-fetch; hash decides whether tabs reload | possible hot reload |
| Console worker restarts | type re-registration **replays every live binding** and drains parked intents (`iii/engine/src/trigger.rs:311-375`) | asset registry **and** subscriber set rebuilt from replay, in arbitrary interleaving; sync + incremental pushes converge every tab ([§ The subscription type](#the-subscription-type-consoleassets)) | hash dedupe absorbs the overlap |
| Worker registers while console is down | intent **parked** in `pending_triggers`, activated on type registration (`RegisterTriggerOutcome::Deferred`, `iii/engine/src/trigger.rs:40-49,435-459`) | delivered on connect | loads then |
| Engine restarts | registry wiped (in-memory `DashMap`s, `iii/engine/src/trigger.rs:200-210`) | console + workers reconnect and re-register; parking makes ordering irrelevant | reconnect + re-seed |

Two deliberate consequences:

- **Use the SDK Message path, not `engine::register_trigger`.** Three reasons.
  Function-path triggers are deliberately durable (`worker_id: None` — "removed
  only by explicit `engine::unregister_trigger`",
  `iii/engine/src/workers/engine_fn/mod.rs:1615-1634`): they'd outlive their
  worker (broken UI pointing at a dead content function) *and* silently vanish
  on engine restart with no replayer. And their ack semantics degrade past the
  10s window (fail-open with the rejection error dropped — see
  [Acks](#acks-and-rejection)), whereas Message-path rejections always reach
  the registrant via the forwarded `TriggerRegistrationResult`. Note the
  durability point corrects
  [`engine-register-trigger-metadata.md`](../engine-register-trigger-metadata.md)
  §5.2 step 3, whose `_caller_worker_id` scoping claim is stale against
  shipped code.
- **Parked registrations are invisible** to
  `engine::registered-triggers::list` (it iterates only live triggers,
  `iii/engine/src/workers/engine_fn/mod.rs:664-682`) and produce no Deferred
  ack. Acceptable for v1: the asset appears when the console does.

## Discovery

Anyone (including the console SPA's Workers page) can enumerate injected UI
through the existing surface — one reason `config` stays small:

```rust
// engine::registered-triggers::list { trigger_type: "console:script" }
// iii/engine/src/workers/engine_fn/mod.rs:280-293
pub struct RegisteredTriggerSummary {
    pub id: String,
    pub trigger_type: String,
    pub function_id: String,
    pub worker_name: String,   // the engine's join from function_id to the worker
                               // serving it (worker_name_for_function_id,
                               // iii/engine/src/workers/engine_fn/mod.rs:676) —
                               // equal to the registrant under the
                               // <worker>::ui-content convention; the engine
                               // records no registrant identity anywhere
    pub config: Value,         // { "path": ... }
    pub config_summary: String,
}
```

The authoritative *loadable* set, though, is the console's own manifest
(`console::ui-manifest`, [hot-reload.md § The manifest](hot-reload.md#the-manifest-debug-surface)),
since only the console knows fetch results and hashes.

## Engine touchpoints

None required. One optional, additive nicety: extend the install-hint table
`KNOWN_TRIGGER_TYPE_PROVIDERS` (`iii/engine/src/trigger.rs:16-28`) with
`("console:script", "console")`, `("console:style", "console")` and
`("console:assets", "console")` so a parked `console:script`
registration logs "install the console worker" instead of the generic
workers.iii.dev hint.

## Security & trust model

**Injected scripts are code execution in the console origin, on purpose.** The
trust boundary of an iii deployment is the engine connection itself: any
connected worker can already invoke any function. Injectable UI extends that
existing trust to the console's DOM — it does not create a new class of
principal. No iframe, no permissions model, v1.

Facts a deployer should know (all verified in source, none new to this
feature):

- With no RBAC configured, **every** engine connection — including browser
  tabs through the console's `/ws` proxy — may register functions, trigger
  types, and triggers (`AuthResult` defaults,
  `iii/engine/src/workers/worker/rbac_session.rs:76-86`, attached to every
  main-port connection, `iii/engine/src/engine/mod.rs:1478-1529`).
- Trigger-type ownership is **last-writer-wins with replay**: whoever registers
  the type id last becomes the registrator and receives every existing binding
  (`iii/engine/src/trigger.rs:311-375`). A hostile page could re-register
  `console:script` and intercept registrations.
- `Message::UnregisterTrigger` has **no ownership check** — any connection can
  tear down any trigger by its enumerable id
  (`iii/engine/src/engine/mod.rs:886-899`).
- The console serves no CSP today (nothing in `web/index.html` or the axum
  handlers), so `import()` of same-origin URLs is unrestricted.

Posture:

1. **Local-trust by default.** The console binds for a developer's own engine;
   everything on the bus is already root-equivalent within the deployment.
2. **Cheap proxy hardening (recommended, same pattern as today's one proxy
   mutation).** The `/ws` proxy already rewrites browser `registerfunction`
   frames (`stamp_internal_registration`,
   `workers/console/src/proxy.rs:142-164`); it should additionally **drop**
   browser-originated `registertriggertype` frames — the SPA never sends one
   (verified: no `registerTriggerType` caller in `web/src`), so this breaks
   nothing and closes tab-originated type hijack through the proxy.
   Worker-originated hijack remains an RBAC concern.
3. **Hardened deployments** gate `allow_trigger_type_registration` /
   `allowed_trigger_types` via RBAC — the
   [rbac-proxy spec](../2026-06-22-rbac-proxy-worker/README.md) is the standing
   answer; this spec adds no new mechanism.
4. **Kill switch:** `injectable_ui: false` in the console's `config.yaml`
   (default `true`) skips trigger-type registration, the `/ui` + `/vendor`
   routes, and the SPA loader (the manifest function returns an empty,
   `disabled: true` response). One flag next to `http_port`
   (`workers/console/src/config.rs:14-39`).

## Testing

- **Handler unit tests** (pure Rust, mirror `workers/memory` handler tests):
  path validation matrix; override supersede including the
  unregister-echo no-op; replay idempotence (same id + hash → no publish);
  unknown-id unregister no-op; id reuse across paths (same id re-registered
  under a different path → old path removed + `delete` pushed, no
  `engine::unregister_trigger` issued); interleaving — two concurrent
  registrations for one path commit in arrival order regardless of fetch
  completion order (exercises the serialization queue); style lint — an
  unscoped `body { }` rule yields a manifest warning, a fully scoped sheet
  yields none; subscriptions — record → immediate `sync` with the current
  registry, later commits fan out to all recorded subscribers, unregister
  (and disconnect GC) drops the subscriber, a subscription recorded mid-replay
  receives partial `sync` + incremental pushes that converge.
- **Wire tests**: registration through a real engine → ack observed;
  `Err` from the handler produces `trigger_registration_failed` on the
  function path and late-unwind removal on the Message path.
- **Serve tests** (axum, alongside the existing router tests that construct
  `server::router` directly — note the router state widens, a mechanical
  breaking change to those tests, `workers/console/src/server.rs:22-33`):
  `/ui/*path` MIME + `no-cache` + ETag/304; 404 unknown path; `/ui` manifest
  shape; kill switch removes routes.
- **e2e** (workers repo `e2e/`): fixture worker registers `fixture/page.js`;
  fixture subscriber registers `console:assets`; assert the `sync` push on
  subscription, manifest content, served bytes, a `set` push on re-register
  with changed content, a `delete` push on worker disconnect.
