---
name: iii
description: WebSocket-routed worker mesh. Authoring guide for the engine's function and trigger model, the SDK surface, worker::* lifecycle ops (add, remove, update, start, stop, list, clear, schema), and the patterns for deploying workers (registry vs sandbox). Single self-contained skill — meant for system-prompt injection; do not re-fetch.
---

# iii

iii is a WebSocket-routed worker mesh. One engine process (default port `49134`) holds a live registry of every connected worker, every function those workers expose, and every trigger bound to them. Workers are independent OS processes that open a WebSocket to the engine and register **Functions** (`service::name` handlers) and **Triggers** (the events that invoke those Functions). There is no direct worker-to-worker traffic — every call routes through the engine, which makes language, runtime, and physical location of any given worker invisible to its callers.

**You extend yourself by writing iii workers.** Three lines get you on the bus:

```ts
import { registerWorker } from 'iii-sdk'
const iii = registerWorker(process.env.III_ENGINE_URL!, { workerName: 'demo' })
iii.registerFunction('demo::add', async ({ a, b }: { a: number; b: number }) => ({ c: a + b }))
```

The instant the handshake completes, `demo::add` is callable from any worker (and the harness itself) via `iii.trigger({ function_id: 'demo::add', payload: { a: 2, b: 3 } })`. No restart, no registration with the harness — the engine routes it automatically.

**Trust contract for this page.** Names below were verified against `iii-sdk@0.13.0+`, `workers/iii-permissions.yaml`, and the engine's native introspection (`engine::functions::*`, `engine::triggers::*`, `engine::workers::*`). When training-data recall disagrees with this page, this page wins. When this page disagrees with the live registry on a specific worker's current shape, the live registry wins — `directory::registry::workers::info { name: '<provider>' }` is the supersession point cited throughout.

## Before authoring — check what already exists

Always run these two reads before writing a worker. The capability you need may be one call away.

- `engine::functions::list` — every function currently registered with this engine, across every connected worker. Filter with `prefix` or `search`.
- `directory::registry::workers::list` — every worker published in the public registry. If the registry has it, prefer `worker::add { source: { kind: "registry", name: "..." } }` over re-implementing it.

Only reach for hand-authored workers when both come up empty.

## The four primitives

| Primitive | What it is | Owned by |
|---|---|---|
| Engine | One coordinator process. Routes every invocation. | The operator |
| Worker | A process that opens a WebSocket to the engine. | Anyone who writes one |
| Function | A named handler inside a worker, id `service::name`. Stable across worker restarts. | The registering worker |
| Trigger | A `(type, config, function_id)` triple. Causes a function to run when an event fires. | A worker (the type-publisher) + a caller (the binding) |

Three consequences worth internalising:

1. **No worker-to-worker traffic.** Every call is `worker → engine → worker`. Workers never address each other directly. Location and language are invisible.
2. **No restart coordination.** Restarting a worker is invisible to callers as long as it re-registers the same function ids. Two workers registering the same function id = automatic load-balance.
3. **No polling unless you opt in.** Triggers are the engine's push channel. The engine fans events out to bound functions when the underlying source fires.

The function id is the only contract between any two workers.

```mermaid
graph TD
  Harness["harness (LLM worker)"] <-->|"WS"| Engine["iii engine :49134 (registry + router)"]
  Engine <-->|"WS"| WorkerA["agent-authored worker (Node sandbox) my::new-fn"]
  Engine <-->|"WS"| Http["iii-http"]
  Engine <-->|"WS"| Cron["iii-cron"]
  Engine <-->|"WS"| State["iii-state"]
  Engine <-->|"WS"| Sb["iii-sandbox"]
  HttpReq["HTTP request (curl, browser)"] -->|"HTTP :3111"| Http
```

Every edge to the engine is a WebSocket. The only non-WS edge is external traffic into `iii-http`, which terminates the public HTTP protocol and translates it into engine traffic.

## Authoritative names — the single canonical table on this page

Pass these literal strings as `type:` in `iii.registerTrigger({ type, function_id, config })`. Anything else fails at the engine before any handler runs.

| `type:` | Provider worker | Canonical config keys |
|---|---|---|
| `http` | `iii-http` | `{ http_method: 'POST', api_path: '/foo' }` |
| `cron` | `iii-cron` | `{ expression: '0 */5 * * * * *' }` — **7 fields: sec min hr dom mon dow year** |
| `queue` | `iii-queue` | `{ queue: 'orders', retries: 3 }` |
| `state` | `iii-state` | `{ scope: 'agent', condition_function_id: 'svc::cond' }` |
| `stream` | `iii-stream` | `{ stream_name: 'agent::events' }` |
| `<your-type>` | any worker that calls `registerTriggerType` | whatever you publish |

Three names training data gets wrong: `webhook` (use `http`), `state-change` (use `state`), 5-field cron (use 7). And one config-key mistake that bites silently: `{ method, path }` for `http` — the `registerTrigger` call **succeeds** at the engine but the route never binds, so `web::fetch` returns 404 while `functions::list` shows the function. Use `{ http_method, api_path }`. This silent failure is row 1 of [Common silent failures](#common-silent-failures-recognise-from-the-symptom).

**Live read that supersedes this table.** Provider versions drift. When in doubt, call:

```jsonc
// directory::registry::workers::info { name: 'iii-http' }
// → README documents the canonical config keys, ApiRequest / ApiResponse shapes, and version.
```

This is the authoritative source the table above was drafted against. Run it before shipping any non-trivial registration. `engine::triggers::info { id: 'http' }` returns the trigger's request/response JSON Schema on engines that expose that introspection surface; fall through to `directory::registry::workers::info` if it returns `function_not_found`.

### Env vars

| Variable | Use it for | Notes |
|---|---|---|
| `III_ENGINE_URL` | Engine WebSocket URL inside a worker | Canonical name. Read with `process.env.III_ENGINE_URL`. When deploying via the sandbox worker, the loopback-rewrite mechanics (which field, which boot phase) live in its README — see [Run it](#run-it). |
| `OTEL_ENABLED=false` | Disable OpenTelemetry export | Set if exporter spam pollutes logs. |
| Anything else (`GITHUB_TOKEN`, etc.) | Worker-specific secrets | Pass through the same env-injection mechanism as `III_ENGINE_URL`. |

`III_URL` is accepted as an alias by some code paths but `III_ENGINE_URL` is the name to standardise on.

### Probing HTTP from the agent loop — `web::fetch`, not `shell::exec curl`

When the agent loop needs to make an HTTP request (smoke-testing a route, calling an external API, scraping a JSON endpoint), call `web::fetch`. It returns `{ ok, status, headers, body }` as a structured envelope, enforces server-side size / timeout caps, and blocks SSRF (RFC1918, cloud metadata `169.254.169.254`, link-local, ULA, multicast). Loopback (`localhost`, `127.0.0.1`, `::1`) is allowed so the harness can hit `iii-http` on `localhost:3111`.

```jsonc
// web::fetch — JSON POST
{
  "url": "https://api.example.com/items",
  "method": "post",                    // case-insensitive
  "json": { "title": "hi" },            // auto-stringifies + sets content-type
  "response_format": "json"             // auto-parses into .json
}
// → { ok, status, headers, body, json, response_format, bytes_truncated }
```

Branch on `ok`, then on `error.code` when `ok:false`: `invalid_payload`, `invalid_url`, `blocked_host`, `timeout`, `too_large`, `too_many_redirects`, `transport_error`. Match on code, not message — messages may reword between releases.

## The TypeScript SDK in one page

```ts
import {
  registerWorker,      // factory; opens WS synchronously from your code's perspective
  TriggerAction,       // .Void() | .Enqueue({ queue })
  IIIInvocationError,  // typed error thrown by iii.trigger()
  Logger,              // OTel-aware structured logger; falls back to console.*
  http,                // narrow-use HTTP-handler wrapper — see "HTTP endpoint pattern"
} from 'iii-sdk'

const iii = registerWorker(process.env.III_ENGINE_URL!, {
  workerName: 'my-worker',                  // appears in engine::workers::list
  invocationTimeoutMs: 30_000,
  reconnectionConfig: { maxRetries: -1 },   // -1 = infinite (the default)
  otel: { enabled: true },                  // OTEL_ENABLED=false also disables
})

// Publish a function. Same handler shape regardless of how the invocation arrives.
const ref = iii.registerFunction(
  'svc::do-thing',
  async (payload) => ({ ok: true }),
  { description, request_format, response_format },  // JSON-Schema-shaped metadata
)
ref.id            // 'svc::do-thing'
ref.unregister()  // drop just this function, keep WS open

// Invoke. Three modes — same method, different `action`.
await iii.trigger({ function_id, payload, timeoutMs })
await iii.trigger({ function_id, payload, action: TriggerAction.Void() })
await iii.trigger({ function_id, payload, action: TriggerAction.Enqueue({ queue }) })

// Bind a function to an event.
iii.registerTrigger({ type, function_id, config })

// Publish a new event source other workers can bind to.
iii.registerTriggerType({ id, description }, { registerTrigger, unregisterTrigger })

await iii.shutdown()  // graceful close; engine evicts functions immediately
```

`registerWorker(url, options?)` opens the WebSocket **synchronously from your code's perspective** — there is no separate `await connect()` step. The returned handle queues calls until the handshake lands.

Schemas in `registerFunction` options (`description`, `request_format`, `response_format`) are JSON-Schema-shaped. **The engine does not reject mismatched payloads at runtime today** — schemas are metadata only. Always declare them: they surface in `engine::functions::info`, drive the console UI, document the contract for the next caller, and reserve a slot for future runtime validation hooks.

### The three invocation modes

| `action` | Caller blocks? | Retries? | Returns | Use when |
|---|---|---|---|---|
| (omitted) | yes | no | the function's result | you need the value to continue |
| `TriggerAction.Void()` | no | no | `null` | one-way notification, no result needed |
| `TriggerAction.Enqueue({ queue })` | no | yes (via `iii-queue`) | `{ messageReceiptId }` | slow or unreliable work; engine handles retry + back-pressure |

### Errors

- **Throw / reject inside the handler** — propagates to the caller as `IIIInvocationError` with the stack trace in the wire payload. Catch it explicitly: `try { await iii.trigger(...) } catch (e) { if (e instanceof IIIInvocationError) { e.code; e.function_id; e.stacktrace } }`. Use for unexpected failures: bad input the handler can't recover from, downstream service crash, programmer mistakes.
- **Return a structured error value** (`{ ok: false, reason: '...' }`) — the call succeeds, the caller sees the error in the return shape. Use for expected failures: validation, "not found", business rules.

Rule of thumb: if a retry might succeed, throw. If a retry will deterministically fail the same way, return a structured value.

### Lifecycle

- `iii.shutdown()` flushes pending traffic and closes the WebSocket. The engine sees `disconnected` and evicts the worker's functions immediately. In-flight invocations against this worker resolve as `invocation_stopped` to their callers.
- `ref.unregister()` on a `FunctionRef`, `Trigger`, or `TriggerTypeRef` removes that one registration without affecting others.
- The SDK reconnects automatically with exponential backoff (configurable via `reconnectionConfig`). Registrations are replayed verbatim on reconnect — do **not** re-register manually. Callers see `invocation_stopped` during the disconnect window; treat it as cancellation, not transient failure.

### Less-known surface

- **`Logger`** — `new Logger().info('msg', { key: val })`. Captures active trace/span context automatically; falls back to `console.*` if OTel is off. Use instead of `console.log` in workers.
- **`createChannel(bufferSize?)`** — returns `{ writer, reader, writerRef, readerRef }`. Pass refs in a `trigger()` payload to stream binary or large structured data between workers without buffering through the engine.
- **`registerFunction(id, httpInvocationConfig, options)`** — the second arg can be `{ url, method, timeout_ms, headers, auth }` instead of a handler, to proxy a Lambda / Cloudflare Worker / external HTTP endpoint as if it were a local function.

## HTTP endpoint pattern

> **Default to plain `async (payload) => result` handlers.** Reach for `http()` only when you need raw `res.status()` / header control AND you are certain the function will never be invoked via `iii.trigger()` (including smoke tests, cross-worker calls, agent harness verification).

The trap: an `http()`-wrapped handler called via direct `iii.trigger()` succeeds-with-wrong-shape — the handler receives the raw payload as `req`, mutates a fake `res` that goes nowhere, returns `undefined`. Smoke tests look broken while the function "works over HTTP."

### Plain handler (default): dual-callable via HTTP AND `iii.trigger()`

`iii-http` delivers an **`ApiRequest`** to the handler:

```
{ path, method, path_params, query_params, body, headers, trigger, context }
```

Read POST body from `payload.body.<field>`, URL params from `payload.path_params.<name>` (`/todos/:id`), querystring from `payload.query_params.<key>`. Names are `path_params` / `query_params` — not `params` / `query`.

Return an **`ApiResponse`**:

```
{ status_code, body, headers? }
```

A bare object yields 500 — the engine treats anything else as malformed.

```ts
iii.registerFunction(
  'api::create-todo',
  async (payload: { body?: { title?: string } }) => {
    const title = payload?.body?.title?.trim()
    if (!title) return { status_code: 400, body: { ok: false, error: 'title required' } }
    return { status_code: 201, body: { ok: true, id: '...', title } }
  },
)

iii.registerTrigger({
  type: 'http',
  function_id: 'api::create-todo',
  config: { http_method: 'POST', api_path: '/todos' },
})

// Callable three ways with the SAME handler:
//   web::fetch  { url: 'http://localhost:3111/todos', method: 'POST',
//                 headers: {'content-type':'application/json'}, body: '{"title":"hi"}' }
//   curl -X POST /todos -H 'Content-Type: application/json' -d '{"title":"hi"}'
//   iii.trigger({ function_id: 'api::create-todo', payload: { body: { title: 'hi' } } })
```

### `http()` (narrow): HTTP-only

```ts
import { http } from 'iii-sdk'

// ⚠️ This function is now ONLY callable via HTTP. Direct iii.trigger() calls
//    will receive undefined-shaped responses.
iii.registerFunction('api::create-todo', http(async (req, res) => {
  res.status(201).json({ id: '...', title: req.body.title })
}))
```

## Custom trigger types — the deepest leverage

`registerTriggerType` publishes a *type* of event source other workers can bind to. Your worker keeps a `{ trigger_id → function_id, config }` table in memory and walks it whenever its underlying source fires.

```ts
type FsWatchConfig = { path: string; recursive?: boolean }
const bindings = new Map<string, { function_id: string; config: FsWatchConfig }>()

iii.registerTriggerType<FsWatchConfig>(
  { id: 'fs::watch', description: 'Fires when a file under `path` changes.' },
  {
    async registerTrigger({ id, function_id, config }) {
      bindings.set(id, { function_id, config })
      startWatchingPath(id, config)        // your fs.watch / chokidar setup
    },
    async unregisterTrigger({ id }) {
      stopWatchingPath(id)
      bindings.delete(id)
    },
  },
)

function onFileChange(triggerId: string, eventPath: string) {
  const binding = bindings.get(triggerId)
  if (!binding) return
  iii.trigger({
    function_id: binding.function_id,
    payload: { path: eventPath, triggerId },
    action: TriggerAction.Void(),
  })
}
```

This is the deepest leverage point in iii: one worker turns its native event source (a webhook hit, a file change, a Slack message, a row update) into something the entire bus can react to **without polling**. From the caller's perspective, your custom type is indistinguishable from a built-in one. See [Recipe 2](#recipe-2--publish-a-custom-trigger-type-fswatch-no-polling) for the full worker.

## Run it

Two paths to get a worker on the bus. Try registry first.

| Path | When | How |
|---|---|---|
| **Registry — `worker::add`** | The worker is already in the public registry. | One call (below). |
| **Authored — sandbox the source yourself** | You wrote the source and need to run it. | The `iii-sandbox` worker is the deployment surface. **Read its canonical API from the directory worker before authoring** (below). |

### Registry — `worker::add`

```jsonc
// worker::add
{ "source": { "kind": "registry", "name": "image-resize", "version": "0.1.2" }, "wait": true }
```

One call installs from the registry or OCI, writes `iii.config.yaml`, caches under `~/.iii/managed/{name}/`, pins `iii.lock`, and (with `wait: true`) blocks until ready. Full lifecycle semantics — source variants, W-codes, consent rules, on-disk artifacts — are in [Worker lifecycle — `worker::*` ops](#worker-lifecycle--worker-ops) below.

### Authored — read the sandbox surface from the directory worker

The `iii-sandbox` worker is the deployment surface for code you wrote yourself: it boots the runtime, accepts source-file writes, and execs processes that open a WebSocket back to the engine. **Don't trust training-data recall for its call shapes** — function names, config keys, env-injection mechanics, exec-serialisation rules, and the detached-launch pattern needed to avoid agent-gateway timeouts all drift between versions. Fetch the canonical surface from the directory worker at the start of an authoring session:

```jsonc
// directory::registry::workers::info { name: "iii-sandbox" }
// → README documents the full lifecycle surface: boot the runtime, write
//   source into the sandbox filesystem, exec processes, read logs, stop.
//   It also documents the env-injection field, the loopback-rewrite phase,
//   exec-serialisation behaviour, and the detached-launch pattern.
```

Pair the README with the markdown skill the sandbox worker shipped — but **skill ids do not mirror worker names.** The `iii-sandbox` worker's skill is at `sandbox/index`, not `iii-sandbox/index`; the directory strips the `iii-` prefix. Never call `directory::skills::get` with an id you predicted from the worker name. Discover it first:

```jsonc
// directory::skills::index { }
// → one paragraph per worker — cheapest discovery surface
// directory::skills::list { search: "iii-sandbox" }
// → returns rows of { id, title, score }; the top score is the right id
// directory::skills::get { id: "<id returned by list — copy verbatim>" }
```

If `directory::skills::get` returns `D110 not_found` anyway, the response body itself carries the fix: `fix.suggestions: [{ id, score, title }]` is the right id with a confidence score, and `next_actions` points back at `directory::skills::list` / `index`. Read those — do not retry `get` with the same id.

If `info` and `skills` both come up empty (older engine, embedded build), fall back to `engine::functions::list { prefix: "sandbox::" }` to discover the surface live.

Once the worker is running, verify it joined the bus — see [Verify the worker came up](#verify-the-worker-came-up) below.

### Pre-flight: confirm the trigger provider is connected

Before binding `http` / `cron` / `queue` / `state` / `stream`, confirm a worker is publishing that trigger type. Trigger registrations against a missing provider succeed **silently** — the binding lands in the registry but never fires.

Try in order, first available wins:

| Call | Healthy when |
|---|---|
| `engine::triggers::list` | `id: '<type>'` appears in the response |
| `worker::list` | `{ name: 'iii-http', status: 'running' }` (managed workers including daemon-supervised providers) |
| `engine::workers::list` | a worker whose `triggers` include `<type>` (WS-connected workers only; may lag a polling cycle) |

`worker::list` is the most reliable in the harness — `engine::*` lists are optional / lag-prone, while `worker::list` reflects the worker-ops daemon's authoritative view. If the provider isn't there: `worker::add { source: { kind: 'registry', name: 'iii-http' }, wait: true }`.

### Verify the worker came up

```jsonc
// engine::functions::list  (filter with `prefix: 'github::'` etc.)
// → look for your function id in the response
```

`functions::list` is **authoritative** — a worker is callable the instant its functions appear there, earlier and more reliably than `workers::list`. Confirm end-to-end with a runtime probe:

```jsonc
// iii.trigger({ function_id: 'github::ping', payload: {} })
// → { ok: true }
```

## Worker lifecycle — `worker::*` ops

> **Callable ids:** `worker::add`, `worker::remove`, `worker::update`, `worker::start`, `worker::stop`, `worker::list`, `worker::clear`, `worker::schema` — pass these to `agent_trigger { function: "worker::add" }` (NOT the skill path from `directory::skills::list`; that's documentation, not a function id).

Install, run, and uninstall workers owned by the `iii-worker-ops` daemon (auto-spawned as an engine sidecar). Every op is also callable as `iii worker <cmd>` on the CLI; the trigger surface documented here is the SDK path that other workers and the engine itself use.

`worker::add` is the single entry point for getting a worker on disk: registry slugs and full OCI refs both flow through it, write the entry to `iii.config.yaml`, cache the artifact under `~/.iii/managed/{name}/`, and pin the resolved version in `iii.lock`.

| Question | Use this |
|----------|----------|
| Install a worker from registry or OCI | `worker::add` |
| Drop config entries (keep cache) | `worker::remove` |
| Re-pin registry workers to latest semver | `worker::update` |
| Spawn a configured worker | `worker::start` |
| Gracefully halt a running worker | `worker::stop` |
| See installed + running state | `worker::list` |
| Wipe cached artifacts (keep config) | `worker::clear` |
| Fetch JSON Schemas for every op | `worker::schema` |

Reach for `worker::list` before any other op when you don't already know what's installed.

### Fetch the live schema, don't trust this page

For exact parameter and response shapes, call:

```jsonc
// engine::functions::info { function_id: "worker::add" }
// → request_format / response_format / description JSON
// Or, equivalently, this surface's self-introspection:
// worker::schema { function_id: "worker::add" }
// → carries default_timeout_ms and idempotent hints alongside the JSON Schemas
```

The schemas the engine returns are the contract. This section covers **only** the cross-cutting behaviors the schemas can't express — source variants, consent rules, error envelope, and which ops touch which on-disk artifact.

### Functions

- `worker::add` — install a worker; writes `iii.config.yaml`, caches under `~/.iii/managed/{name}/`, pins `iii.lock`. Idempotent — `status` distinguishes `installed` / `already_current` / `repaired` / `replaced`.
- `worker::remove` — drop a worker's `iii.config.yaml` entry. **Leaves cached artifacts on disk** — pair with `worker::clear` to reclaim space.
- `worker::update` — re-resolve registry workers against latest semver and rewrite `iii.lock`. **OCI-sourced workers are skipped** (no semver to re-resolve). Does not touch `iii.config.yaml`.
- `worker::start` — spawn a configured worker. Default `wait: true` blocks until the engine sees the WS handshake. Starting an already-running worker is a silent no-op.
- `worker::stop` — graceful shutdown. Destructive; requires `yes: true`. Does **not** touch config / lock / cache.
- `worker::list` — union of `iii.config.yaml` entries, `~/.iii/managed/` artifacts, and running processes the daemon sees. Pure read, safe to poll.
- `worker::clear` — delete cached artifacts under `~/.iii/managed/{name}/`. Does **not** touch config or lock. Requires `yes: true`.
- `worker::schema` — JSON Schemas for every op plus `default_timeout_ms` and `idempotent` hints. Self-introspection.

### Behaviors that aren't in the JSON schemas

The traps below are not visible from `engine::functions::info` and bite agents composing calls from the schema alone.

#### `worker::add` accepts three source variants — one of them is CLI-only

| Variant | Shape | Where it works |
|---|---|---|
| Registry | `{ "kind": "registry", "name": "image-resize", "version": "0.1.2" }` | trigger + CLI |
| OCI | `{ "kind": "oci", "reference": "ghcr.io/iii-hq/node:latest" }` | trigger + CLI |
| Local | `{ "kind": "local", "path": "./builds/image-resize" }` | **CLI only — returns W102 via the trigger surface** |

The harness cannot side-load arbitrary local code through `worker::add`; vendor it into a sandbox via `iii://sandbox` and let it open a WebSocket from there. Missing or malformed `source` returns **W101**; a registry name not in the registry returns **W110**.

#### W-codes — the error envelope

Every `worker::*` op returns errors as `{ "type": "WorkerOpError", "code": "Wxxx", "message": "...", "details": {...} }`. Recurring codes:

- **W100** InvalidName — name doesn't match `[a-z0-9_-]{1,64}`.
- **W101** InvalidSource — `worker::add` got a malformed `source` envelope.
- **W102** LocalPathNotAllowedViaTrigger — `kind: "local"` over the trigger surface.
- **W103** MissingTarget — batch ops (`remove`, `clear`) got empty `names` + unset `all`, or both set.
- **W104** ConsentRequired — destructive op (`remove`, `stop`, `clear`) needs `yes: true`.
- **W110** NotFound — worker name unknown to the registry or unknown locally.
- **W900** Internal — OCI pull / network / filesystem / signal failure. Surface the `details` field; this is the only `W900` body that varies.

**W codes are independent of the S codes used by [`sandbox::*`](iii://sandbox)** — don't conflate them.

#### Destructive ops need explicit consent

`worker::remove`, `worker::stop`, and `worker::clear` all require `yes: true` (exactly the boolean, not `"true"` or `1`). Anything else returns **W104**. This is the only guard against accidentally dropping a running worker's config or a multi-GB cache.

#### Batch ops: `names` XOR `all`

`worker::remove` and `worker::clear` take either a non-empty `names` list **or** `all: true` — never both, never neither (**W103**). Invalid names in `names` return **W100**; valid-but-absent names are silently skipped (so a second `remove` returns an empty `removed` list, the idempotent shape).

#### Which on-disk artifacts each op touches

| Op | `iii.config.yaml` | `iii.lock` | `~/.iii/managed/{name}/` | Running process |
|---|---|---|---|---|
| `worker::add` | write | pin | download | unchanged (use `start`) |
| `worker::remove` | delete entry | unchanged | **unchanged** | engine watcher tears down sandbox |
| `worker::update` | unchanged | rewrite changed pins | download new artifacts | unchanged |
| `worker::start` | unchanged | unchanged | unchanged | spawn |
| `worker::stop` | unchanged | unchanged | unchanged | graceful shutdown |
| `worker::clear` | unchanged | unchanged | **delete tree** | unchanged |

The split matters because reclaiming disk and dropping config are separate intents: `remove` keeps the cache so a re-add is a no-op; `clear` keeps the config so a future start triggers a fresh download.

#### `worker::list` includes builtins; `engine::workers::list` doesn't

`iii-stream`, `iii-http`, and other daemon-managed builtins return `pid: null` and **do not open a WebSocket to the engine**. They won't appear in `engine::workers::list` even when serving traffic. Merge `worker::list` ∪ `engine::workers::list` by `name` for the complete picture `iii-directory`.

#### `worker::update` only re-pins registry workers

OCI-sourced workers are skipped (no semver to re-resolve). To force an OCI re-pull, use `worker::add { force: true }` with the same `source` envelope.

#### CLI mirror

Every op above is also `iii worker <cmd>` on the command line (`iii worker add`, `iii worker list`, etc.). The trigger surface and the CLI share the same daemon, the same lock file, and the same consent rules — calling either path from either side is safe and produces the same observable state. The CLI is also the only place `kind: "local"` works.

## Trust runtime probes over introspection

`engine::triggers::list`, `engine::registered-triggers::list`, `engine::workers::list`, and `engine::functions::list` can return empty for three blurred reasons: (1) older engine doesn't expose that introspection surface, (2) the read came from a store that lags live state, (3) genuinely nothing is registered. **Disambiguate with a runtime probe** — `iii.trigger(...)` against the function, or `web::fetch` against the HTTP route. If the probe succeeds, the registration is live regardless of what `*::list` reports.

Session `v7h9jbk8` hit case (2): both `triggers::list` and `registered-triggers::list` empty but `web::fetch` returned 200 OK on every CRUD endpoint — the routes were bound, the introspection just lied. Don't unbind / re-register on the strength of an empty `*::list` alone; you'll churn a working worker.

## Common silent failures (recognise from the symptom)

The first four rows are silent: SDK calls succeed, `functions::list` looks healthy, while `web::fetch` returns 404 / 500. Check these first when an HTTP worker "should work" but doesn't.

| Stderr / behaviour | Cause | Fix |
|---|---|---|
| `registerTrigger({ type: 'http', ... })` returns OK, `registered-triggers::list` stays empty, `triggers::info { id: 'http' }` returns "trigger type not found" | The `http` trigger TYPE isn't registered on the engine. `iii-http` may be running but didn't publish its type at startup (stale install, dep mismatch, crash mid-handshake). | `worker::list` for `iii-http`. If absent: `worker::add { source: { kind: 'registry', name: 'iii-http' }, wait: true }`. If running: `worker::remove` + `worker::add` to force a fresh handshake. Verify with `triggers::info { id: 'http' }`. |
| HTTP smoke test 404s, `registered-triggers::list` empty, `functions::list` shows the function | `registerTrigger` config used `{ method, path }` instead of `{ http_method, api_path }`. SDK accepted it; route never bound. | Change every `config` to `{ http_method, api_path }`. |
| HTTP request returns 500 or empty body though the function exists | Handler returned a raw object; `iii-http` expects an `ApiResponse` envelope `{ status_code, body, headers? }`. | Wrap the return: `return { status_code: 200, body: { ok: true, ...result } }`. |
| `payload.title` / `payload.query` undefined when called via HTTP | iii-http delivers `ApiRequest { path, method, path_params, query_params, body, headers, trigger, context }` — not body fields at the top level. | Read `payload.body.<field>` (POST/PATCH), `payload.path_params.<name>` (`/todos/:id`), or `payload.query_params.<key>`. |
| `http()` handler returns undefined / breaks on direct `iii.trigger()` call | `http()` wraps the handler in `(req, res) => void`. A direct trigger passes a raw payload, not a req/res pair. | Default to plain `async (payload) => result`. Use `http()` only when you need raw `res.status()` / header control AND no caller will use `iii.trigger()`. |
| `engine rejected trigger type 'webhook'` / `'state-change'` | Wrong trigger type name. | Use `'http'` / `'state'`. See [Authoritative names](#authoritative-names--the-single-canonical-table-on-this-page). |
| `{ "error": "function_not_found", "function": "iii::trigger" }` from the agent loop | `iii::trigger` is the SDK method workers use *inside* a worker process. No meta-tool on the bus takes `{ function_id, payload }` and dispatches. | From the agent loop, set `function` to the function id directly (e.g. `todo::create`) and pass function-specific JSON as `payload`. The list of callable ids is `engine::functions::list`. Session `ue3l0zqk` hit this 5× before pivoting. |
| Worker process exited immediately after `registerFunction` returned; nothing appears on the bus | The handler-registering process must stay alive — `registerWorker` opens a WS but doesn't block. | Keep the process alive: long-running handler, `setInterval`, or `await new Promise(() => {})`. For sandbox-deployed workers, see the detached-launch pattern in the `iii-sandbox` README. |
| Sandbox-deployed worker's stdout shows `engine=ws://127.0.0.1:49134` AND `"Worker registered and ready!"` but `engine::functions::list` stays empty | `III_ENGINE_URL` was set on `sandbox::exec` env, not `sandbox::create` env. The daemon's loopback rewrite is boot-only — exec env is NOT rewritten, so `127.0.0.1` resolves to the guest's own loopback, not the host. The worker logs the URL it *tried* to connect to before the WS handshake fails. **Your stdout is lying.** | Stop the sandbox. Recreate with `env: ["III_ENGINE_URL=ws://127.0.0.1:49134"]` passed to `sandbox::create`. After re-launch, the worker's stdout should show `engine=ws://100.96.0.x:49134` (gateway IP, not loopback) and `functions::list` should populate within 1–2 s. Session `c007a1a7` hit this and burned ~10 turns. |
| `engine::workers::list` empty but `functions::list` shows the function | `workers::list` polling lag — updates after `functions::list`. | Trust `functions::list` as authoritative; `workers::list` is a secondary check. |
| BOTH `engine::functions::list` AND `web::fetch` against the route fail | Worker isn't really connected. Don't go further down the diagnostic ladder until you've confirmed the worker is on the bus. | Read the worker's stdout: `engine=` should be the gateway IP (e.g. `100.96.0.x`), not `127.0.0.1`. If it shows loopback, the env was passed to `exec` instead of `create` — see the row above. |

Failure modes that are sandbox-deployment-specific (loopback reachability, env-injection phase, exec-gateway timeouts, exec serialisation) are documented in `directory::registry::workers::info { name: "iii-sandbox" }` — read it once when you start authoring rather than rediscovering each row from a stack trace.

## Diagnostic ladder — HTTP route registered but not firing

When `iii.registerTrigger({ type: 'http', ... })` returned OK but `web::fetch` returns 404, walk top-down. Each step rules out the most common cause before the next.

| Step | Call | Proves the layer is healthy when |
|---|---|---|
| 1. Function reached the engine | `engine::functions::list { prefix: 'todo::' }` | Your function id appears with the worker name. |
| 2. Trigger binding reached the engine | `engine::registered-triggers::list { function_id: 'todo::create' }` | One row per `registerTrigger` call, with the config you passed. |
| 3. The trigger TYPE exists | `engine::triggers::info { id: 'http' }` | Returns `{ id: 'http', trigger_request_format, ... }`. "trigger type not found" → step 4. |
| 4. The type provider is connected | `worker::list` | `iii-http` appears with `status: 'running'`. |
| 5. Re-init the provider | `worker::remove { name: 'iii-http' }` then `worker::add { source: { kind: 'registry', name: 'iii-http' }, wait: true }` | After re-add, step 3 returns the type. Type providers publish their types on connect; missing type despite running worker = handshake didn't complete or manifest is stale. |
| 6. Config keys match the provider | `directory::registry::workers::info { name: 'iii-http' }` README | `http_method`/`api_path`; ApiResponse `{ status_code, body }`. |

Session `h8cxe1u3` hit step 3: `registerTrigger` returned OK (the SDK accepts any type string), `worker::list` showed `iii-http` running, but `triggers::info { id: 'http' }` returned "trigger type not found" — the provider hadn't published its type. `worker::remove` + `worker::add` re-handshakes it.

## Discovery surface

| Call | Returns |
|---|---|
| `engine::functions::list` | Every function currently registered across all workers. Filter with `prefix` or `search`. |
| `engine::functions::info { function_id }` | One function's `description`, `request_format`, `response_format`, owning worker. |
| `engine::workers::list` | Every WS-connected worker. Misses daemon-supervised providers (`iii-http`, `iii-cron`, `iii-state`) that don't open an engine WS — for those, also call `worker::list`. |
| `engine::triggers::list` | Every trigger TYPE published (legal `type:` values). |
| `engine::triggers::info { id }` | One trigger type's `trigger_request_format` / `call_request_format`. |
| `engine::registered-triggers::list` | Every trigger INSTANCE currently bound. Filter by `function_id` or `worker`. |
| `worker::list` | Daemon-managed worker view. Authoritative for `iii-http` / `iii-cron` / `iii-state`. |
| `directory::registry::workers::list` | Every worker published in the public registry. |
| `directory::registry::workers::info { name }` | A registry worker's README — canonical config / ApiRequest / ApiResponse shapes, version-pinned. |
| `directory::skills::list { search: function_id }` + `directory::skills::get { id }` | The markdown skill a function's worker shipped (when one exists). The how-to that `engine::functions::info` doesn't carry. |

## Two patterns worth one canonical example each

The third-party-wrap and custom-trigger-type patterns are already complete in [The TypeScript SDK in one page](#the-typescript-sdk-in-one-page) and [Custom trigger types](#custom-trigger-types--the-deepest-leverage). The two patterns below are not — both are load-bearing and not covered earlier.

### Ephemeral one-shot worker (register → run → shutdown)

A worker can register, do its work inline, call `shutdown()`, and exit. The function is callable while the job runs; the engine evicts it from the registry on disconnect, so the status function disappears the moment the work is done — the correct post-condition. Use this for batch jobs, Kubernetes Jobs, or any one-off where progress should be observable without reinventing IPC.

```ts
// workers/etl/worker.ts
import { registerWorker } from 'iii-sdk'
const iii = registerWorker(process.env.III_ENGINE_URL!, { workerName: 'etl-2026-05' })

let state: { phase: 'pending' | 'extracting' | 'transforming' | 'loading' | 'done'; rows: number } =
  { phase: 'pending', rows: 0 }
iii.registerFunction('etl-2026-05::status', async () => state)

;(async () => {
  state = { phase: 'extracting', rows: 0 };     const rows = await extract()
  state = { phase: 'transforming', rows: rows.length }; const out = await transform(rows)
  state = { phase: 'loading', rows: out.length };       await load(out)
  state = { phase: 'done', rows: out.length }
  await iii.shutdown(); process.exit(0)
})()
```

Scope `workerName` and the function id to the job's identity (`etl-2026-05`) so multiple runs coexist without collision. For long-lived jobs that should survive a restart, publish the worker and deploy via `worker::add` (daemon respawns on failure) instead.

### Dual-callable HTTP function (and the smoke test that proves it)

Use plain `async (payload) => result` handlers — not the `http()` wrapper — so HTTP routes and direct `iii.trigger()` calls share one implementation. Read `ApiRequest` fields (`body`, `path_params`, `query_params`) and return an `ApiResponse` envelope.

**Before you write the handlers**: if you're deploying this in a sandbox, the worker MUST be booted with `III_ENGINE_URL` in the `sandbox::create` env array (not the `sandbox::exec` env). The sandbox daemon's loopback rewrite is boot-only; setting it on exec silently fails. See `sandbox` "Reaching the host engine" + the matching row in the silent-failures table above.

```ts
const todos = new Map<string, { id: string; title: string }>()

iii.registerFunction('todo::create', async (req: { body?: { title?: string } }) => {
  const title = req?.body?.title?.trim()
  if (!title) return { status_code: 400, body: { ok: false, error: 'title required' } }
  const id = crypto.randomUUID(); todos.set(id, { id, title })
  return { status_code: 201, body: { ok: true, todo: todos.get(id) } }
})

iii.registerFunction('todo::get', async (req: { path_params?: { id?: string } }) => {
  const todo = todos.get(req?.path_params?.id ?? '')
  if (!todo) return { status_code: 404, body: { ok: false, error: 'not found' } }
  return { status_code: 200, body: { ok: true, todo } }
})

iii.registerTrigger({ type: 'http', function_id: 'todo::create', config: { http_method: 'POST', api_path: '/todos' } })
iii.registerTrigger({ type: 'http', function_id: 'todo::get',    config: { http_method: 'GET',  api_path: '/todos/:id' } })
```

**The dual-mode smoke test.** Always verify both call paths — failing one silently while the other works is the exact bug `http()` introduces:

```jsonc
// HTTP path:
//   web::fetch { url: 'http://localhost:3111/todos', method: 'POST',
//                headers: {'content-type':'application/json'}, body: '{"title":"x"}' }
// Direct trigger — payload mirrors the ApiRequest shape iii-http delivers:
//   iii.trigger({ function_id: 'todo::create', payload: { body: { title: 'x' } } })
//   iii.trigger({ function_id: 'todo::get',    payload: { path_params: { id: 'abc' } } })
```

- HTTP works but `iii.trigger()` returns undefined-shaped → handler is wrapped in `http()`; remove the wrapper.
- `iii.trigger()` works but HTTP 404s → trigger config used `{ method, path }`; switch to `{ http_method, api_path }`.
- Both 500 with empty body → handler returned a bare object; wrap as `{ status_code, body }`.

## Anti-patterns

- **Polling instead of trigger types.** A function on a cron reading a queue / database / file every N seconds is almost always wrappable as a custom trigger type. See [Custom trigger types](#custom-trigger-types--the-deepest-leverage) for the model.
- **Reinventing existing workers.** Run `engine::functions::list` and `directory::registry::workers::list` before authoring. The most common harness mistake is wrapping something that already exists.
- **Side-channel state between workers.** Use `iii-state` (`state::*` functions) for shared key/value state. Don't write workers that read each other's files or hit each other's HTTP endpoints — every cross-worker call goes through `iii.trigger`.
- **Catching the wrong error type.** `iii.trigger()` throws `IIIInvocationError`. Catch that specifically; broad `catch (e)` loses `e.code` / `e.function_id` / `e.stacktrace`.
- **Trusting introspection over runtime probes.** Empty `*::list` can mean lag, not absence. A successful `iii.trigger()` or `web::fetch` is the authoritative signal — see [Trust runtime probes over introspection](#trust-runtime-probes-over-introspection).
