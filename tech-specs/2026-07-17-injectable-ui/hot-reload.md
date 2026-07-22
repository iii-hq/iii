# Hot reload — Vite semantics without Vite

The console UI must treat an updated `console:script`/`console:style` trigger the way a Vite
dev server treats a changed module: invalidate, re-import, re-wire — without
shipping Vite in the console. This document specifies the serving side of the
console-worker registry (the registry structure and override algorithm are
defined in
[injection-protocol.md § Override semantics](injection-protocol.md#override-semantics-same-path--override))
and the browser loader, and closes with an honest fidelity table against
Vite's HMR.

## Identity, versions, hashes

Every asset is identified by its **path** and versioned by its **content
hash**: `hash = hex(sha256(content))`, abbreviated to the first 16 hex chars
everywhere it travels. There is no counter.

Why hash, not counter: the browser's module map is append-only for the life of
the page — `import(url)` for an already-seen URL returns the cached instance,
and nothing evicts it. A monotonic counter resets when the console worker
restarts while tabs keep their module maps, so `?v=2` after a restart could
alias a *different* `?v=2` already imported — a silent stale-module bug. The
hash also gives replay dedupe for free: re-delivered registrations with
unchanged content publish nothing.

## Serving: two new axum routes

Today's router is exactly `/`, `/assets/*path`, `/ws` with a 404 fallback
(`workers/console/src/server.rs:22-33`), so new paths are free. Router state
widens from `Arc<String>` (engine URL) to an `AppState { engine_url, ui_registry }` —
a mechanical change that also touches `serve()` and the tests that call
`router()` directly.

| Route | Serves | Headers |
|---|---|---|
| `GET /ui/*path` | current bytes for a registered asset, from the in-memory registry | `Content-Type` from asset kind; `Cache-Control: no-cache`; `ETag: "<hash>"` (304 on `If-None-Match`) |
| `GET /ui` | the manifest JSON (same shape as `console::ui-manifest`) — curl-friendly debugging | `no-cache` |
| `GET /vendor/*path` | the shared-dep shim modules ([slots-and-api.md](slots-and-api.md)), embedded in `web/dist` like everything else | `text/javascript`; `Cache-Control: no-cache` |

Deliberately **not** under `/assets/*`: that route is reserved for Vite's
content-hashed immutable files (`Cache-Control: public, max-age=31536000,
immutable`, `workers/console/src/assets.rs:19-32,49-61`) — mutable injected
assets must never inherit that header. `no-cache` + ETag is correct and cheap:
the `?v=<hash>` query is what actually busts the module map; the HTTP cache is
just not allowed to lie.

## The push channel: `console:assets`

No stream worker, no extra endpoint: the push channel is the third
console-owned trigger type. A tab subscribes by registering a
`console:assets` trigger whose `function_id` is its own per-tab handler; the
console worker invokes that handler — a plain bus call relayed over the
tab's existing `/ws` connection — for every asset event. Registration
mechanics, ordering guarantees, and lifecycle live in
[injection-protocol.md § The subscription type](injection-protocol.md#the-subscription-type-consoleassets).

**Tab subscription:**

```ts
const off = client.on(UI_ASSETS_FN, onPush)                     // iii::console::ui-assets
const offTrigger = client.registerTrigger({
  type: 'console:assets',
  function_id: `${UI_ASSETS_FN}::${client.browserId}`,          // per-tab handler id
  config: {},
})
```

**Push payloads** (the tab handler's input; `kind` is the short asset kind —
type ids are `console:*`, kinds stay `script`/`style`):

```jsonc
{ "event": "sync",   "assets": [ { "path": "state/page.js", "kind": "script", "hash": "9f2b6c01d4e8aa17" }, … ] }
{ "event": "set",    "path": "state/page.js", "kind": "script", "hash": "…" }
{ "event": "delete", "path": "state/page.js", "kind": "script", "hash": "…" }   // echoes the stored data
```

The handler id keeps the `iii::` prefix so pushes are span-suppressed like the
existing trace-feed handlers (`is_iii_builtin_function_id`,
`iii/engine/src/workers/telemetry/mod.rs:202-220`), and the `/ws` proxy stamps
it `metadata.internal = true` like every browser registration
(`workers/console/src/proxy.rs:142-164`) — during a UI dev loop the developer
is *staring at the traces page*; rebuild-loop pushes must not spam it.

The subscriber set is in-memory console state, rebuilt from engine replay
exactly like the asset registry — nothing in this channel is a store; the
manifest below is a debug window, not a second source of truth.

## The manifest (debug surface)

New console function (internal, like `console::status`):

| | |
|---|---|
| Function id | `console::ui-manifest` |
| Input | `{}` |
| Output | `{ "disabled": bool, "assets": [ { "path", "kind": "script"\|"style", "hash", "worker": string\|null, "warnings": [string] } ] }` |

`worker` is best-effort attribution joined from
`engine::registered-triggers::list { trigger_type }`
(`iii/engine/src/workers/engine_fn/mod.rs:280-293`). Note what `worker_name`
actually is: the engine's join from the trigger's `function_id` to the worker
serving that function — which the `<worker>::ui-content` convention makes
equal to the registrant; the owner callback itself never learns the
registrant. `null` when the join fails. `warnings` carries the style lint's
findings ([injection-protocol.md § Style lint](injection-protocol.md#style-lint-warn-only));
empty for `script` assets and for clean styles.

Tab boot is one step: **register the `console:assets` trigger**. The console
answers with a `sync` push, and the loader diffs it against loaded state —
new/changed hash ⇒ (re)load (styles applied before scripts), missing path ⇒
dispose. Every later `sync` (browser-SDK reconnect replay, console restart)
goes through the same diff; hash dedupe makes replays no-ops. There is no
subscribe-then-seed ordering to get right and no seed/frame race to argue
about: sync-on-subscribe closes the boot-time gap by construction
([injection-protocol.md § The subscription type](injection-protocol.md#the-subscription-type-consoleassets)
— the stream-worker draft of this design needed a careful subscribe-first
rule exactly here).

`console::ui-manifest` stays as the curl-friendly debug surface (and the
`GET /ui` route's body) — the loader itself never calls it.

## The browser loader

All asset URLs resolve against the **document base**, never the site root: the
console explicitly supports being mounted at an arbitrary subpath (Vite
`base: './'` "so the embedded SPA works behind a reverse proxy",
`workers/console/web/vite.config.ts:8-42`), and the SPA already solves this
exact problem for its WS endpoint (`new URL('./ws', window.location.href)`,
`workers/console/web/src/lib/iii-client.ts:263-274`). The loader computes

```ts
const base = new URL('.', document.baseURI)
```

once and resolves every asset URL against it. A module-relative specifier
would *not* work: the loader lives in a Vite chunk under `assets/`, so a bare
`import('./ui/…')` would resolve to `assets/ui/…`. (Server-side route notation
like `GET /ui/*path` elsewhere in this spec describes the axum route shape,
not browser URL resolution.)

Loader state, per tab:

```ts
type LoadedScript = {
  path: string
  hash: string
  cleanups: Array<() => void>   // every host registration + any setup() teardown
}
type LoadedStyle = { path: string; hash: string; link: HTMLLinkElement }
```

### `script` load / reload

```text
on sync(assets):
  for each loaded path missing from assets: dispose + forget
  for each asset: fall through to set(path, hash) below
on set(path, hash):
  if loaded[path]?.hash == hash: return            // dedupe (replay, reconnect sync)
  if loaded[path]: dispose(path)                   // run cleanups LIFO; clear slot entries
  mod = await import(new URL(`ui/${path}?v=${hash}`, base).href)
  if typeof mod.default != 'function': fail(path, 'no default setup() export')
  host = makeHost(path)                            // per-script scoped registrar (slots-and-api.md)
  teardown = await mod.default(host)               // may return a cleanup fn
  loaded[path] = { path, hash, cleanups: host.cleanups + [teardown] }
on delete(path): dispose(path); delete loaded[path]
```

Failure containment (`fail`): an `import()` rejection, a missing default
export, or a `setup()` throw logs `console.error` and surfaces one non-fatal
toast naming the path — and, on a *reload*, the previous version's
registrations were already disposed, so the slot contributions simply drop out
until the next good version arrives. A broken extension never takes the
console down (render-time crashes are separately fenced per slot entry with
the existing `ErrorBoundary`, `workers/console/web/src/components/ui/ErrorBoundary.tsx:8-41`).

Old module instances necessarily leak in the module map (no eviction API
exists) — identical to Vite dev, accepted.

### CSS: `style` triggers

Vite's own link-swap technique, verbatim:

```text
on set(path, hash), no link yet:   append <link rel="stylesheet" data-iii-ui="{path}"
                                          href={new URL(`ui/${path}?v=${hash}`, base).href}>
on set(path, hash), link exists:   create the new link; on its `load` event,
                                   remove the old one                           // no FOUC
on delete(path):                   remove the link
```

Injected CSS naturally themes with the console: the design tokens are plain
CSS custom properties (`--color-bg`, `--color-accent`, … under `@theme`, with
`[data-theme="dark"]` overriding the same variables,
`workers/console/web/src/index.css:12-74`), so `var(--color-accent)` in a
worker's stylesheet follows light/dark switching for free. Scoping under
`data-iii-ui` is a compile-side contract
([authoring.md § Styling & Tailwind](authoring.md#styling--tailwind)) — the
serving path and the link-swap above are agnostic to it.

## End-to-end: one hot edit

```mermaid
sequenceDiagram
  participant D as dev editing ui/page.tsx
  participant W as state worker (esbuild --watch)
  participant E as engine
  participant C as console worker
  participant T as each open tab

  D->>W: save file → rebuild page.js
  W->>E: registerTrigger(script, {path:"state/page.js"}, state::ui-content)
  E->>C: RegisterTrigger (forwarded to type owner)
  C->>W: state::ui-content {path} → {content}
  C->>C: sha256 → hash changed; supersede old trigger id; engine::unregister_trigger(old)
  C->>T: invoke iii::console::ui-assets::<browserId> {event:set, path, kind, hash} — per subscriber
  Note over C,T: a plain bus call relayed over each tab's /ws; span-suppressed via the iii:: prefix
  T->>T: dispose old cleanups
  T->>C: GET /ui/state/page.js?v=<hash>
  T->>T: import → setup(host) → slots repopulate (~one frame later)
```

## Vite fidelity: what we mimic, what we don't

| Vite HMR concept | Vite mechanism (model knowledge) | This design | Divergence? |
|---|---|---|---|
| Update push | `vite-hmr` WS, `{type:'update', updates:[{path, timestamp}]}` | the console invokes the tab's `iii::console::ui-assets` handler with `{event, path, kind, hash}` — a bus call over the existing `/ws` | transport swapped; same shape of signal |
| Cache busting | `import(path + '?t=' + timestamp)` | `import(new URL('ui/' + path + '?v=' + hash, base).href)` | hash instead of timestamp — restart-proof, dedupes no-ops |
| HMR boundary | nearest module with `import.meta.hot.accept()` | **every script is its own boundary** — `setup(host)` re-runs whole | simpler by construction; no propagation walk needed |
| Dispose hooks | `import.meta.hot.dispose(cb)` | host-tracked cleanups + optional `setup()` return value | equivalent, structural instead of opt-in |
| Unacceptable update (dead-end propagation, entry-HTML change) | server sends `{type:'full-reload'}` when no boundary accepts | cannot occur — every script is its own accepting boundary and the console shell is never hot-swapped; this design **never** issues a full page reload | **deliberate divergence** — a full reload of the console (live traces, in-flight chat) is worse than one missing extension |
| Failed update import/execution | catches the error, logs `[hmr] Failed to reload …`, page keeps running | `console.error` + non-fatal toast; contributions drop out until the next good version | none — same drop-out behavior (Vite's server-error overlay is replaced by the toast) |
| `prune` | `{type:'prune'}` when a module disappears | `delete` push → dispose | same |
| CSS update | clone `<link>` with `?t=`, remove old on `load` | identical, `?v=<hash>` | none |
| State preservation | react-refresh (`$RefreshReg$` transform) | none — slot subtrees remount | **deliberate divergence**, see README non-goals |
| Heartbeat | `ping`/`pong` | none — the SDK owns reconnection (infinite backoff+jitter, `iii/sdk/packages/node/iii-browser/src/iii-constants.ts:32-58`) + re-seed on reconnect | covered by transport |
