# PROGRESS — Repo checks

Cross-checking the `ideal-docs` outline against the actual source repositories at `/Users/tony/iii/projects/iii-temp/`. Companion to [`PROGRESS.md`](./PROGRESS.md) (which tracked the source-docs migration). Ground rules from [`project-rules/`](./project-rules/) apply throughout.

## Scope and method

**Repos surveyed:**
- `iii-temp/engine/` — Rust engine, CLI dispatcher, built-in workers, config, protocol.
- `iii-temp/sdk/` — Node, Python, Rust, Browser SDKs.
- `iii-temp/console/` — Console UI (`console-frontend` React) + `console-rust` server.
- `iii-temp/crates/` — Rust crates monorepo (scaffolder, worker manager, sandbox infra, etc.).
- `iii-temp/website/` — Static marketing site (`index.html`, `manifesto.html`).

**Method:** five parallel read-only Explore surveys, each producing a structured findings list, then synthesized here. Spot-checked surprising claims directly. Not file-by-file the way the source-docs migration was — the goal here is alignment drift, not slot-by-slot routing.

## Status legend

- [ ] open finding — needs decision / action
- [x] confirmed in alignment with rules / docs
- [~] partially aligned — see note

## Top-level findings (most actionable first)

1. **`iii Cloud` command exists in the engine but has no docs home.** `engine/src/main.rs:87` defines `Cloud { args }` ("Manage iii Cloud deployments"), dispatching to an `iii-cloud` binary. Neither ideal-docs nor the project rules mention iii Cloud at all. Decide: is this in scope for ideal-docs, a separate product surface, or pre-release that should stay undocumented?

2. **`iii update` (engine self-update) is undocumented.** Distinct from `iii worker update`. `engine/src/main.rs:120` defines `Update { target }` for updating iii and managed binaries. Should land on `using-iii/cli.mdx` or `getting-started/install.mdx`.

3. **`iii sandbox` subcommands are completely unstubbed.** ~~`crates/iii-worker` ships `iii sandbox run/create/exec/list/stop/upload/download` (ephemeral and long-lived sandbox VMs). Currently the only sandbox stub anywhere in ideal-docs was the `using-iii/deployment.mdx` "Run an ephemeral worker" line. Needs explicit stub coverage — likely a new section on `using-iii/workers.mdx` or its own page.~~ **Resolved 2026-05-06:** the `iii sandbox` subcommand is being removed and replaced by `iii trigger sandbox::run` (and other `iii trigger sandbox::*` invocations). All sandbox docs route to Worker Docs (sandbox is a worker). No ideal-docs work needed.

4. **All four SDKs still export forbidden surfaces.** Logger, channels, telemetry options, and stream channel refs are still in the public API of every SDK. Per `sdks.md` and `workers.md` these should be stripped from SDK packages (and they're already stripped from the docs pages). **Resolution 2026-05-06:** docs side already aligned with the rule; flag-and-hand-off to SDK authors — see hanging pieces below for the cleanup punch list. Most extensive in Rust SDK (entire `telemetry` module re-exported in `lib.rs:114-127`).

5. **Browser SDK has type drift between source and exports.** `IIIReconnectionConfig` (in `iii-constants.ts`) and `RegisterFunctionFormat` (in `iii-types.ts:57`) exist in source but are NOT re-exported from `index.ts`. **Resolution 2026-05-06:** stripped both Types entries from `sdk-reference/browser-sdk.mdx`; reworded the Connection lifecycle stub to reference the reconnection options on `InitOptions` rather than the unexported type name. **Needs review:** confirm with SDK authors whether the omission is intentional (types are internal) or accidental (`index.ts` should re-export them). If accidental, re-add the Types entries once the SDK exposes them. (Supersedes the "source-doc gap" entry in [`PROGRESS.md`](./PROGRESS.md) hanging pieces.)

6. **Branding drift: `motia-tools` still ships in `crates/`.** `crates/motia-tools` exposes `motia create` using the same `scaffolder-core` as `crates/iii-tools`, but points at `motia.dev/docs`. **Resolution 2026-05-06:** `project-rules/general.md` now declares `motia-tools` out of scope for ideal-docs (separate product surface, predates the iii rename). Code-side decision (legacy-keep / deprecate / remove from monorepo) deferred to hanging pieces.

7. **`iii trigger` violates the `iii noun verb` rule.** ~~`engine/src/main.rs:62-63` declares `Trigger(TriggerArgs)` as a top-level command (verb-first). Either the command should rename (`iii function invoke` or `iii function trigger`), or `cli.md` rule needs an explicit exemption noted.~~ **Resolved 2026-05-06:** `iii trigger <function-path> [argA="value" argB=5 ...]` is the canonical way to invoke any registered function from the CLI; recognized exemption added to `project-rules/cli.md`.

8. **Adapters are still active in the engine despite being deprecated in docs.** `general.md` says drop adapter content; engine ships `stream/adapters/`, `state/adapters/`, `queue/adapters/`, `pubsub/adapters/`, `cron/adapters/` directories with real implementations, and example configs (`engine/config.yaml`, `config-remote-kv.yaml`) actively configure them. **Resolution 2026-05-06:** `general.md` rule confirmed — adapters are genuinely deprecated; remaining engine-side presence is a code cleanup task. Docs work continues to drop adapter content. See hanging pieces for the engine cleanup punch list.

9. **Stale `iii-config.yaml` references in engine repo.** `engine/README.md:48,49,75,85,133,140`, `engine/src/workers/queue/README.md:59`, `engine/src/workers/rest_api/README.md:93`, `engine/src/workers/worker/README.md:7` all use the old name. **Resolution 2026-05-06:** out-of-repo files; no ideal-docs work. Engine-side cleanup tracked in hanging pieces.

10. **Console README uses `iii-console` (hyphenated) binary name; docs stub says `iii console`.** Both work — `iii console` dispatches to `iii-console` via the engine top-level dispatcher (`engine/src/main.rs:71` `Console { args }`). **Resolution 2026-05-06:** docs always use `iii console`; `iii-console` is the local/internal binary name only and not user-facing. No `using-iii/console.mdx` change needed.

## Engine survey

### CLI surface
**Top-level commands** (from `engine/src/main.rs:62-122`):
- `iii trigger ...` (verb-first — see top finding #7)
- `iii console [args]` → dispatches to `iii-console`
- `iii cloud [args]` → dispatches to `iii-cloud` (UNDOCUMENTED)
- `iii worker [args]` → dispatches to `iii-worker`
- `iii sandbox [args]` → dispatches to `iii-worker sandbox`
- `iii update [target]` (UNDOCUMENTED)
- `iii create [args]` → dispatches to `iii-tools create`
- Engine flags: `--config`, `--use-default-config`, `--version`

**Drift flags:**
- [ ] `iii trigger` violates `iii noun verb` (top finding #7).
- [ ] `iii cloud` has no docs home (top finding #1).
- [ ] `iii update` not in `using-iii/cli.mdx` (top finding #2).
- [x] `iii worker`, `iii sandbox`, `iii console`, `iii create` all dispatch correctly per `cli.md`.

### Engine config
**Schema** (`engine/src/config/mod.rs`, `engine/src/workers/config.rs:30-34`):
- Top-level: `workers:` array of entries with `name`, `image?`, `config?`.
- Per-worker `config` is untyped (`serde_json::Value`).
- `${VAR:default}` env-var expansion supported (config.rs:47-76).
- Default-loaded modules use the `inventory`-registered `enabled_by_default` flag; `mandatory` workers cannot be disabled.

**Examples present:** `engine/config.yaml`, `engine/config-remote-kv.yaml`, `engine/config.prod.yaml`.

**Drift flags:**
- [ ] Adapters still configured in all three example configs (top finding #8).
- [ ] `iii-config.yaml` lingering in `engine/README.md` (top finding #9).
- [x] No `iii-config.yaml` files in tracked engine configs themselves.
- [x] No `iii.worker.yaml` in engine repo — correct, it's a per-worker concern.

### Built-in workers (registration map)
Spot-checked via `register_worker!` macro across the source. Engine survey initially missed these because it was checking `mod.rs`; the registrations live in the worker's main impl file:

| Worker name | Source | Mandatory | Default |
|---|---|---|---|
| `iii-engine-functions` | `workers/engine_fn/mod.rs` | yes | yes |
| `iii-observability` | `workers/observability/mod.rs:598` | yes | yes |
| `iii-telemetry` | `workers/telemetry/mod.rs` | yes | yes |
| `iii-worker-manager` | `workers/worker/mod.rs` | yes | yes |
| `iii-http` | `workers/rest_api/api_core.rs:462` | no | yes |
| `iii-stream` | `workers/stream/stream.rs:829` | no | yes |
| `iii-state` | `workers/state/state.rs:409` | no | yes |
| `iii-cron` | `workers/cron/cron.rs:169` | no | yes |
| `iii-queue` | `workers/queue/queue.rs:803` | no | yes |
| `iii-pubsub` | `workers/pubsub/pubsub.rs:193` | no | yes |
| `iii-exec` | `workers/shell/worker.rs:68` | no | no |
| `iii-bridge` | `workers/bridge_client/...` | no | no |
| `iii-http-functions` | `workers/http_functions/...` | no | no |

**Drift flags:**
- [x] **`iii-bridge`** is registered as a worker via `engine/src/workers/bridge_client/...`. Confirmed as one of the 11 Worker Docs targets per the original PROGRESS.md (`workers/iii-bridge.mdx` flagged for Worker Docs). No ideal-docs stub work needed; not a missing surface.
- [~] **`iii-engine-functions`** — registered as a *worker*, not a discovery surface. The existing "Engine discovery functions" stub on `sdk-reference/engine-sdk.mdx` is documenting this worker's surface. **Resolution 2026-05-06:** "Engine SDK" kept as a friendly user-facing alias; the worker-name reality belongs in Worker Docs. Flagged for engineering review (see hanging pieces).
- [x] **`iii-telemetry`** is distinct from `iii-observability`. Telemetry is Amplitude/usage; observability is OTel. Both are mandatory. **Resolution 2026-05-06:** clarification added to `project-rules/workers.md` ("Logger and telemetry belong to iii-observability" section now spells out the two-worker split). Worker Docs has two separate targets.
- [~] **`iii-http-functions`** vs **`iii-http`**: two separate HTTP-related workers — `iii-http` (the REST API worker, `enabled_by_default=true`) and `iii-http-functions` (`enabled_by_default=false`). **Resolution 2026-05-06:** scope split documented in `project-rules/workers.md` (new section: `iii-http` and `iii-http-functions` are separate workers). Two Worker Docs targets. Naming is confusing — flagged for engineering review (see hanging pieces).
- [x] All worker names use `iii-*` prefix consistently.
- [x] No "external vs built-in" distinction in code.

### Engine APIs / discovery functions
From `workers/engine_fn/mod.rs:494-615`:
- `engine::channels::create`
- `engine::functions::list`
- `engine::workers::list`
- `engine::workers::register`
- `engine::triggers::list`
- `engine::trigger-types::list`

Trigger types: `engine::functions-available`, `engine::workers-available`.

From observability worker (`workers/observability/mod.rs`):
- `engine::log::{info,warn,error,debug,trace}`
- `engine::baggage::{get,set,get_all}`
- `engine::traces::{list,tree,clear}`
- `engine::metrics::list`
- `engine::logs::{list,clear}`
- `engine::sampling::rules`
- `engine::health::check`
- `engine::alerts::{list,evaluate}`
- `engine::rollups::list`

**Drift flags:**
- [x] `engine::log::*` and `engine::baggage::*` are observability-worker functions but use the `engine::` prefix. **Resolution 2026-05-06:** punted to iii-observability Worker Docs per `workers.md`. `sdk-reference/engine-sdk.mdx` confirmed to not enumerate them. `engine::` prefix naming flagged for engineering review (see hanging pieces).
- [x] `engine::traces/metrics/logs/sampling/health/alerts/rollups` query functions are observability-worker queries but engine-prefixed. **Resolution 2026-05-06:** same as above — punted to iii-observability Worker Docs. `engine-sdk.mdx` only enumerates iii-engine-functions surface (functions/workers/triggers/trigger-types::list, workers::register, channels::create, plus the two engine triggers).
- [~] `engine::channels::create` is registered in iii-engine-functions (`workers/engine_fn/mod.rs:494-615`) but channels-as-concept belong to iii-worker-manager per `workers.md`. **Resolution 2026-05-06:** code reality kept — `engine-sdk.mdx` lists `engine::channels::create` under the iii-engine-functions surface; channels-as-concept stays with iii-worker-manager Worker Docs. Conceptual-vs-registration split flagged for engineering review (see hanging pieces).

### Trigger types
- All trigger types are defined per-worker (correct per "everything is a worker"). Definitions live near each worker's `register_worker!` call; reusable schemas live in `engine/src/trigger_formats.rs`.
- Engine-emitted: `engine::functions-available`, `engine::workers-available`.
- Per-worker examples: `http`, `cron`, `queue`, `pubsub:subscriber`, `state` (state-changed), `stream:join`, `stream:leave`, `stream`, `log`.

[x] Trigger types correctly distributed; no central enumeration page needed.

### Protocol
- `engine/src/protocol.rs:42-140` defines the `Message` enum (`RegisterFunction`, `RegisterTrigger`, `RegisterTriggerType`, `InvokeFunction`, `InvocationResult`, `Heartbeat`, …).
- `ErrorBody` (lines 175-197), `WorkerMetrics` (142-172), `StreamChannelRef` (207-217), `ChannelDirection` (200-205) all live here.
- The `engine-sdk.mdx` page already stubs Message types / Connection flow / Invocation lifecycle / Register-and-result messages — just no detail behind those headings. Auto-gen target.

[ ] `StreamChannelRef` and `ChannelDirection` are protocol-level types that surface in SDKs. Per `workers.md`, channels belong to iii-worker-manager Worker Docs; the protocol-level definitions live in the engine. Worker Docs should reference back to the engine protocol page. Note: this is a conceptual question for the worker-docs split, not an ideal-docs action.

## SDK survey

### Node SDK (`sdk/packages/node/iii/`)
- **Methods:** `registerWorker`, `registerFunction`, `registerTrigger`, `registerService`, `trigger`, `registerTriggerType`, `unregisterTriggerType`, `createChannel`, `createStream`, `shutdown`. Matches `sdk-reference/node-sdk.mdx`.
- **Drift vs docs:** minimal. All 9 methods + types listed.
- **Forbidden surfaces still exported:** `ChannelReader`, `ChannelWriter`, `Logger`, `StreamChannelRef`, `TelemetryOptions` (embedded in `InitOptions` as `otel?`).

### Python SDK (`sdk/packages/python/iii/`)
- **Methods:** `register_worker`, `register_function`, `register_trigger`, `register_service`, `trigger`, `trigger_async`, `shutdown`, `shutdown_async`, `register_trigger_type`, `unregister_trigger_type`, `create_channel`, `create_stream`. Plus `get_connection_state` (NOT in stub).
- **Drift vs docs:**
  - [~] `get_connection_state` exists in source (`iii.py:678`) — not in `python-sdk.mdx`. **Resolution 2026-05-06:** leave undocumented pending SDK-author confirmation that the method is a primary surface (not an internal introspection helper). Stub-or-skip decision deferred to hanging pieces.
  - [x] `trigger_async` / `shutdown_async` correctly noted with "differs from node" markers.
- **Forbidden surfaces still exported:** `ChannelReader`, `ChannelWriter`, `Logger`, `StreamChannelRef`, `OtelConfig`, `TelemetryOptions`.

### Rust SDK (`sdk/packages/rust/iii/`)
- **Methods (in code, beyond what's in docs):** `set_headers`, `set_metadata`, `set_otel_config`, `get_connection_state`, `register_function_with` (builder).
- **Drift vs docs:**
  - [~] `set_headers` (`iii.rs:776`) — present in code, not in `rust-sdk.mdx`. **Resolution 2026-05-06:** deferred to SDK-author confirmation (primary surface or internal helper?). Tracked in hanging pieces.
  - [~] `set_metadata` (`iii.rs:771`) — same. Deferred to SDK-author confirmation.
  - [~] `set_otel_config` (`iii.rs:781`) — telemetry-related. **Resolution 2026-05-06:** deferred to SDK-author confirmation; if kept, strip per observability rule (add to forbidden-surface cleanup list).
  - [~] `get_connection_state` (`iii.rs:1182`) — covered by S1 hanging piece.
  - [x] `register_function_with` (`iii.rs:951`) — Rust-only builder variant. **Resolution 2026-05-06:** previously flagged for removal from the SDK; added to the Rust forbidden-surface cleanup punch list. No docs stub.
- **Forbidden surfaces still exported:** Far more extensive than other SDKs. `lib.rs:114-127` re-exports the entire `telemetry` module (`init_otel`, `shutdown_otel`, `with_span`, all baggage/traceparent helpers, `SpanKind`, `Status` from OTel itself). Plus all channel types (`ChannelDirection`, `ChannelItem`, `ChannelReader`, `ChannelWriter`, `StreamChannelRef`, `extract_channel_refs`, `is_channel_ref`), `Logger`, `OtelConfig`, `ReconnectionConfig`.

### Browser SDK (`sdk/packages/node/iii-browser/`)
- **Methods:** mirror Node SDK: `registerWorker`, `registerFunction`, `registerTrigger`, `registerService`, `trigger`, `registerTriggerType`, `unregisterTriggerType`, `createChannel`, `createStream`, `shutdown`.
- **Drift vs docs:**
  - [ ] **CRITICAL** (top finding #5): `IIIReconnectionConfig` and `RegisterFunctionFormat` exist in source but are NOT in `index.ts` exports. The browser-sdk.mdx stub lists both. Either re-export them or remove from docs.
  - [x] `HttpAuthConfig` and `HttpInvocationConfig` — listed in old source-doc references; not exported from browser SDK (browser doesn't support HTTP-invoked functions the same way Node does). **Confirmed 2026-05-06:** `browser-sdk.mdx` does not claim either type. Aligned with SDK reality.
- **Forbidden surfaces still exported:** `ChannelReader`, `ChannelWriter`. `TelemetryOptions` embedded in `InitOptions`. Browser SDK correctly does NOT export `Logger` or `OtelConfig`.

### Engine SDK page (`sdk-reference/engine-sdk.mdx`)
- Discovery-functions stub aligned with engine-survey findings (six functions plus two trigger types).
- [x] Decide: enumerate observability worker functions (`engine::log::*`, `engine::traces::*`, etc.) on this page, or leave entirely to iii-observability Worker Docs? **Resolution 2026-05-06:** Worker Docs only. `engine-sdk.mdx` confirmed to enumerate only the iii-engine-functions surface; observability functions punted entirely to iii-observability Worker Docs. `engine::` prefix naming flagged for engineering review (see hanging pieces).

### Cross-SDK alignment status (refresh on PROGRESS.md hanging pieces)
- [x] Python `ReconnectionConfig` ≡ Node/Browser `IIIReconnectionConfig` (one engine-WS config). Naming difference real.
- [x] Rust has no user-facing engine-WS reconnection config. Reconnection internal. Confirmed.
- [x] Python's `trigger_async`/`shutdown_async` real; Node lacks both; Rust has `shutdown_async` and a natively-async `trigger`. Confirmed.
- [ ] **NEW:** Browser SDK omits exports for `IIIReconnectionConfig` and `RegisterFunctionFormat` (top finding #5).
- [ ] **NEW:** Rust SDK has 5 methods missing from `rust-sdk.mdx` stub (above).
- [~] **NEW:** Python SDK has `get_connection_state` missing from `python-sdk.mdx` stub. Node SDK has internal `IIIConnectionState` tracking but no public `getConnectionState()`. Same hanging piece as S1.

## Console survey

### Frontend pages / nav
Source: `console/packages/console-frontend/src/routes/*.tsx` and `src/components/layout/Sidebar.tsx:26-38,82-86`.

**Sidebar order:**
Workers → Functions → Triggers → States → Streams → Queues → Traces → Logs → Config. Plus **Flow** (inserted after Workers when `enableFlow=true`).

[x] Matches `using-iii/console.mdx` stubs.

### Flow feature flag
- CLI: `--enable-flow`. Env: `III_ENABLE_FLOW`. (`console-rust/src/main.rs:48-49`.)
- [x] Matches `console.md` rule.

### Ports (default)
| Port | Use | Source |
|---|---|---|
| 3111 | engine HTTP | `console-rust/src/main.rs:28` |
| 3112 | engine WS | `console-rust/src/main.rs:32` |
| 3113 | console UI | `console-rust/src/main.rs:16` |
| 49134 | SDK bridge WS | `console-rust/src/main.rs:36` |

[x] All match `console.md`.

### Backend (console-rust)
- Axum server. Embeds the React SPA (rust-embed). Reverse-proxies `/api/engine/*` and `/ws/streams` to the engine. Registers itself as an iii worker via the SDK bridge. Exposes `engine::console::*` functions for console-specific endpoints.
- Env vars: `OTEL_DISABLED`, `OTEL_SERVICE_NAME`, `III_ENABLE_FLOW`. CLI flags mirror env vars.

### Launch
- Binary: `iii-console` (hyphenated). Also reachable via the `iii console` dispatcher in the engine.
- Install: `curl … install.iii.dev/console/main/install.sh`, GitHub Releases, or build from source.
- [ ] Top finding #10: `using-iii/console.mdx` Launch stub should mention both forms (`iii console` and `iii-console`).

### UI page → worker mapping
| UI page | Backing function(s) | Worker |
|---|---|---|
| Workers | `engine::workers::list` | iii-worker-manager (engine_fn) |
| Functions | `engine::functions::list` | iii-worker-manager (engine_fn) |
| Triggers | `engine::triggers::list` | iii-worker-manager (engine_fn) |
| States | `state::list_groups`, `state::list`, `state::set`, `state::delete` | iii-state |
| Streams | `stream::list_all` | iii-stream |
| Queues | `engine::queue::list_topics`, `engine::queue::topic_stats`, `iii::durable::publish` | iii-queue |
| Traces | `engine::traces::list/tree/clear` | iii-observability |
| Logs | `engine::logs::list/clear` | iii-observability |
| Flow | persisted via `state::get/set` (`__console.flowConfigs`) | (no worker) |
| Config | `engine::health::check` + adapter / trigger / worker enumeration | iii-worker-manager + iii-observability |

[x] No UI page lacks a worker counterpart, except Flow (which is purely UI state).
[x] No registered worker is missing from the UI surface (cron triggers shown under Triggers; DLQ shown under Queues).

## Crates survey

### Routing summary

| Crate | Type | Routing |
|---|---|---|
| `iii-tools` | Binary (`iii`) | **ideal-docs** — `getting-started/install.mdx`, `getting-started/quickstart.mdx` (the `iii create` scaffolder) |
| `iii-worker` | Binary (`iii worker`/`iii sandbox`) | **ideal-docs** — `using-iii/workers.mdx` (all subcommands) |
| `iii-supervisor` | Library | Internal infra |
| `iii-init` | Library + guest binary | Internal infra (PID 1 in microVM) |
| `iii-network` | Library | Internal infra (smoltcp networking for VM sandboxes) |
| `iii-filesystem` | Library | Internal infra (passthrough FS for VM sandboxes) |
| `iii-shell-proto` | Library | Internal infra (`iii worker exec` wire protocol) |
| `iii-shell-client` | Library | Internal infra (`iii worker exec` host client) |
| `scaffolder-core` | Library | Internal infra (shared by iii-tools / motia-tools) |
| `motia-tools` | Binary (`motia`) | **Outside ideal-docs** — Motia branding (top finding #6) |

### iii-tools (`iii create`)
- Binary name `iii`, single command `create`.
- Modes: interactive TUI (`iii` no args) or non-interactive (`iii create --directory <dir> --template <name> --languages <ts,py> --yes`).
- [x] `getting-started/install.mdx` mentions installing the engine via the script. **Resolution 2026-05-06:** `iii create` is being removed and replaced by `iii project init` (project scaffolding) and `iii worker init` (worker scaffolding). Both default to a base template; `--template <name>` selects a specific template.
- [x] PROGRESS.md "Scaffold the project" stub on `getting-started/quickstart.mdx` updated to name `iii project init`. New "Scaffold a new worker" stub added on `using-iii/workers.mdx` for `iii worker init`. `project-rules/cli.md` example list updated to include both new commands.

### iii-worker (`iii worker`, `iii sandbox`)
Subcommands enumerated:

**`iii worker`:** `add` | `remove` | `reinstall` | `update [name]` | `clear [name]` | `start` | `stop` | `restart` | `list` | `sync` | `verify` | `status` | `logs` | `exec`.

**`iii sandbox`:** `run` | `create` | `exec` | `list` | `stop` | `upload` | `download`.

[x] All commands follow `iii noun verb`.
[ ] Top finding #3: sandbox commands have no docs home in ideal-docs.

### Sandbox surface
The sandbox surface in `iii-worker` is broader than ideal-docs currently captures. Suggested coverage:
- `using-iii/workers.mdx` adds a "Sandbox" section with stubs for `iii sandbox run|create|exec|list|stop|upload|download`.
- Cross-link from `using-iii/deployment.mdx` (already has "Run an ephemeral worker" stub) to the sandbox section.
- Distinguish: `iii sandbox run` (one-shot ephemeral VM, exit on completion) vs `iii sandbox create` (long-lived, manage by ID).

### Branding drift (`motia-tools`)
- `crates/motia-tools/Cargo.toml` points `docs_url` at `motia.dev/docs`.
- Shares `scaffolder-core` and the same templates repo (`iii-hq/templates.git`) with `iii-tools`.
- [ ] Top finding #6. Decide: legacy keep, deprecate, or remove from monorepo.
- [x] Either way, no ideal-docs work for motia-tools.

### Internal-infra crates
`iii-supervisor`, `iii-init`, `iii-network`, `iii-filesystem`, `iii-shell-proto`, `iii-shell-client`, `scaffolder-core` — all internal. No ideal-docs targets. Not user-facing. They surface indirectly via `iii sandbox` (now → Worker Docs per top finding #3 resolution) and `iii worker exec` (already stubbed in `using-iii/workers.mdx`); the crates themselves don't get a docs page. **Confirmed 2026-05-06.**

## Website survey

### Product framing
- Tagline: "Three primitives. Zero integration cost." / "Everything is a worker."
- Three primitives: Worker, Trigger, Function (consistent across site and docs).
- Positioning analogies: Unix everything-is-a-file, React everything-is-a-component, iii everything-is-a-worker.
- [x] `index.mdx` "What makes iii different" stub (six system traits) doesn't mention the quadratic-to-linear integration framing the website leads with. **Resolution 2026-05-06:** pulled the website's hero language directly into the docs landing as the lead body — title rewritten to "unreasonably simple software engineering", three lead paragraphs ported verbatim. The "What makes iii different" traits stub is preserved below the lead.

### Install / onboarding flow
- Website primary command: `curl -fsSL https://install.iii.dev/iii/main/install.sh | sh`.
- Website demo shows `iii worker add iii-state`, `iii worker add iii-http` as next steps.
- [x] Aligns with `getting-started/install.mdx` and `quickstart.mdx` stubs.

### Naming
- Product name: consistently "iii".
- Footer: "Motia LLC" copyright (legal entity, not product name).
- Discord invite: `discord.gg/motia` → rebrand to `discord.gg/iiidev` (see [`todo/WORK.website.md`](./todo/WORK.website.md) "Active items").
- Sample email in console demo: `alex@motia.dev`.
- [x] No iii vs motia drift in product naming.

### Links to docs
- Website nav: `<a href="/docs">docs</a>`.
- Hero CTA: `https://iii.dev/docs/install`.
- Footer: `https://iii.dev/docs/quickstart`.
- Website README confirms `/docs/*` proxies to `iii-docs.vercel.app` via Vercel.
- [x] **URL drift:** website uses `/docs/install` and `/docs/quickstart`; ideal-docs used `/getting-started/install` and `/getting-started/quickstart`. **Resolution 2026-05-06:** docs paths switched to match the website — `getting-started/install.mdx` → `install.mdx` (root), `getting-started/quickstart.mdx` → `quickstart.mdx` (root), `getting-started/` directory removed. `docs.json` "Getting Started" nav group still groups the pages but references them at root paths. No redirects needed.

### Sitemap / SEO
- Sitemap lists 4 URLs (`/`, `/manifesto`, `/llms.txt`, `/AGENTS.md`).
- [x] Docs not in website sitemap by design — separate deployment.

### Voice & tone
- Website: lowercase, declarative, paradigm-shift framing, philosophical.
- Docs: mostly placeholder right now; tone alignment is a post-stub task.
- [x] When authoring docs prose, align with the manifesto's confident-declarative voice rather than tutorial-speak. **Resolution 2026-05-06:** new `project-rules/voice.md` captures the voice rule (declarative, confident, paradigm-shift framing, avoid marketing fluff / tutorial-speak / hedging).

## Hanging pieces (carried over and updated from `PROGRESS.md`)

- **Auto-generated config reference** — still pending. No movement; restoration plan in PROGRESS.md `Hanging pieces` is still the right shape. Open question (per-worker vs cross-cutting) reinforced by this audit: with adapters still active in code (top finding #8), the per-worker split is more defensible.

- **Auto-generated SDK reference docs need pre/post-append slots for narrative content** — unchanged. Expected to be solved alongside config-reference auto-gen.

- **Source-doc gap (browser SDK omits `IIIReconnectionConfig`, `RegisterFunctionFormat`)** — **partially resolved, partially escalated**. Both types exist in source; neither is exported from the browser SDK's `index.ts`. New action: either add re-exports or strip from docs (top finding #5).

- **SDK type-list mismatches (Node vs Python vs Rust)** — re-confirmed from source:
  - Rust still does not expose `IIIReconnectionConfig`. Reconnection is internal. Real gap.
  - Rust still does not expose `RegisterFunctionFormat`. Uses `schemars::JsonSchema` derive. Equivalent capability, different mechanism.
  - Python SDK does export `RegisterFunctionFormat` (matches Node).
  - Python adds: `trigger_async`, `shutdown_async`, `get_connection_state`.
  - Rust adds: `set_headers`, `set_metadata`, `set_otel_config`, `register_function_with`, `get_connection_state`.
  - All four SDKs continue to export forbidden surfaces (logger, channels, telemetry options).

## Vale style rule + cleanup (added 2026-05-06)

Added `lives` and `lives with` to `styles/Terminology/SlopMarketing.yml` (anthropomorphic content-location phrasing — content doesn't *live* anywhere; it's documented, stored, or located). Cleaned up existing usages across `project-rules/cli.md`, `project-rules/workers.md`, `project-rules/general.md`, `project-rules/console.md`, `understanding-iii/engine.mdx`, and `sdk-reference/engine-sdk.mdx`. Standardized replacement phrasing: `is documented with` for Worker Docs callouts (matches existing SDK callout convention in `sdks.md`).

## Disable-telemetry move (added 2026-05-06)

Removed `how-to/disable-telemetry.mdx` (was a placeholder-only stub). Added a `## Disable telemetry` section at the bottom of `sdk-reference/engine-sdk.mdx` that distinguishes `III_TELEMETRY_ENABLED` (iii-telemetry / Amplitude) from `OTEL_ENABLED` (iii-observability / OTel). Updated `docs.json` to remove the page from the How-to nav group.

## Introspection routing (added 2026-05-06)

Source: Slack thread, Mike + Anthony, 2026-05-06 — <https://motia-workspace.slack.com/archives/D0A0C30776K/p1778095916525539>.

Introspection of a running iii instance — listing workers / functions / triggers / trigger types, querying traces / logs / metrics, streaming activity — is critical for humans and agent skills alike. Per `workers.md`, the content belongs in Worker Docs (iii-engine-functions for lists/discovery; iii-observability for traces/logs/metrics). User concern was that this surface is too important to be invisible from ideal-docs.

**Resolution 2026-05-06:**
- New general rule added to `project-rules/workers.md`: broadly-important Worker Docs surfaces get callouts on the relevant ideal-docs pages (signpost, don't duplicate). Generalizes the existing SDK callout pattern.
- Introspection callouts placed on:
  - `understanding-iii/engine.mdx` — under "Discovery and the live registry".
  - `sdk-reference/engine-sdk.mdx` — top-of-page Note.
  - `using-iii/console.mdx` — top-of-page Note framing the console as the visual surface for the underlying APIs.
  - `using-iii/workers.mdx` — "Did you know?" Note pointing at iii-engine-functions, iii-observability, and the console.
- No new `using-iii/introspection.mdx` page; Worker Docs is canonical.

## New hanging pieces from this pass

- **`iii cloud` has no docs home and no project-rule entry.** Not part of `using-iii/`, `understanding-iii/`, `expanding-iii/`, or any worker. Decide scope before writing any docs around it.

- **`engine::log::*` / `engine::traces::*` / observability worker functions are engine-prefixed but worker-owned.** Prefix is misleading. Naming review separate from docs work; flag for engine team. Once resolved, decide whether `engine-sdk.mdx` should enumerate them or punt entirely to iii-observability Worker Docs.

- **`engine::channels::create` registered in iii-engine-functions, not iii-worker-manager — needs engineering review.** Channels-as-concept belong to iii-worker-manager (`workers.md` rule), but the channel-creation discovery function is registered in iii-engine-functions. Engineering should weigh in on whether the registration should move or whether this split is intentional (engine-functions is the discovery-function home regardless of conceptual owner). Docs side reflects code reality.

- **`engine::` prefix on observability-worker functions — needs engineering review.** `engine::log::*`, `engine::traces::*`, `engine::metrics::*`, `engine::logs::*`, `engine::baggage::*`, `engine::sampling::rules`, `engine::health::check`, `engine::alerts::*`, `engine::rollups::list` are all user-facing functions registered by the iii-observability worker but prefixed `engine::`, suggesting "the engine itself" rather than a worker. Engineering should weigh in on whether the prefix should change (e.g., `obs::*`, `iii-observability::*`) or stay as-is. Docs side already routes them to iii-observability Worker Docs per `workers.md`.

- **`iii-http` vs `iii-http-functions` naming — needs engineering review.** Two real workers with confusingly similar names. `iii-http` is the REST API surface; `iii-http-functions` is the outbound HTTP-invocation surface. Engineering should weigh in on whether `iii-http-functions` should rename (e.g., `iii-http-client`, `iii-http-outbound`) for unambiguous user-facing reference. Docs side already disambiguates in `project-rules/workers.md`.

- **`iii-engine-functions` is a worker, not "the engine" — needs engineering review.** Working decision (2026-05-06): keep "Engine SDK" as a friendly user-facing alias on `sdk-reference/engine-sdk.mdx`; the worker-name reality (iii-engine-functions) lives in Worker Docs. Engineering should weigh in on whether this naming split is durable or whether the page should be renamed/refocused once Worker Docs settle. Specifically: does `engine::*` prefix stay even after the worker is renamed? Are discovery functions ever expected to move out of iii-engine-functions?

- **Sandbox surface needs explicit ideal-docs home.** Top finding #3. Recommended: a "Sandboxes" section on `using-iii/workers.mdx` (or its own `using-iii/sandboxes.mdx` page if the surface grows).

- **Engine-side `iii-config.yaml` → `config.yaml` cleanup (hand-off).** `config.md` rule says engine config file is `config.yaml`. Stale `iii-config.yaml` references remain in:
  - `engine/README.md:48,49,75,85,133,140` (first-touch instructions, highest priority).
  - `engine/src/workers/queue/README.md:59`.
  - `engine/src/workers/rest_api/README.md:93`.
  - `engine/src/workers/worker/README.md:7`.
  Engine team punch list. Per-worker READMEs may move to Worker Docs and can be normalized at that point.

- **Engine-side adapter cleanup (hand-off).** `general.md` rule confirmed (adapters deprecated). Engine still ships:
  - `engine/src/workers/stream/adapters/`
  - `engine/src/workers/state/adapters/`
  - `engine/src/workers/queue/adapters/`
  - `engine/src/workers/pubsub/adapters/`
  - `engine/src/workers/cron/adapters/`
  - Adapter blocks in `engine/config.yaml` (lines 6-13, 17-21, 150-153, 157-158, 160-163), `engine/config-remote-kv.yaml` (lines 7-9), and `engine/config.prod.yaml`.
  Engine team punch list. No docs work; ideal-docs continues to drop adapter content.

- **`iii update` and `iii cloud` first-class commands lack docs targets.** Both top-level. Both have engine-side dispatchers. Decide before stubbing.

- **`motia-tools` legacy/active status.** Decide before any future docs pass.

- **Introspection may consolidate into a dedicated worker (engineering review).** Currently introspection is split across iii-engine-functions (lists / discovery / channels) and iii-observability (traces / logs / metrics / alerts). User flagged that this may need to "improve" and "be moved to a worker instead of from iii itself." Engineering should weigh in on whether to consolidate the surfaces into a dedicated introspection worker, or keep the current split and improve discovery within Worker Docs.

- **Streaming traces / logs / events as triggers (Worker Docs hand-off).** User-flagged example: a trigger that accepts traces or logs as input is the reactive primitive for live-event consumption. Worker Docs (iii-observability) should cover this pattern explicitly — both the consumer side (subscribe to log events) and the producer side (emit logs that trigger downstream functions). Important for humans, critical for agent skills.

- **iii agent skill bundles should reference iii-engine-functions and iii-observability Worker Docs.** Existing skill bundles (`iii-getting-started`, `iii-browser-sdk`, `iii-http-middleware` — see CLAUDE.md skills_system) should link to introspection content once Worker Docs land. Skill authoring is the right place for the agent-side introspection guide; ideal-docs only signposts.

- **`iii create` rename in flight (engine-side hand-off).** `iii create` (currently shipped via `crates/iii-tools`) is being removed and replaced by two new commands: `iii project init [--template <name>]` for project scaffolding and `iii worker init [--template <name>]` for worker scaffolding. Both default to a base template when `--template` is omitted. Docs already updated to the new commands (`getting-started/quickstart.mdx`, `using-iii/workers.mdx`, `project-rules/cli.md`); engine-side dispatcher and `iii-tools` package still need code changes to match.

- **SDK `get_connection_state` — primary surface or internal helper?** Python (`iii.py:678`) and Rust (`iii.rs:1182`) expose `get_connection_state`. Node and Browser do not. SDK authors should confirm whether this is intended as a user-facing primary surface (in which case stub on python-sdk.mdx and rust-sdk.mdx, and add to Node/Browser for parity) or an internal introspection helper (in which case keep undocumented).

- **Rust SDK `set_headers` / `set_metadata` / `set_otel_config` — primary surface or internal?** All three are public on `sdk/packages/rust/iii/src/iii.rs` (lines 776, 771, 781) but absent from `rust-sdk.mdx`. SDK authors should confirm scope: if user-facing, stub them with reviewer notes (and consider Node/Python parity); if internal, mark with `pub(crate)` or remove from public exports. `set_otel_config` is additionally subject to the observability-stripping rule and joins the forbidden-surface cleanup list if kept public.

- **SDK packages still export forbidden surfaces (cleanup hand-off).** Docs pages are aligned with `sdks.md` (logger / channels / telemetry stripped, callouts in place pointing to iii-observability and iii-worker-manager). The packages themselves still export these symbols. SDK authors' punch list:
  - **Node** (`sdk/packages/node/iii/src/index.ts`): drop `ChannelReader`, `ChannelWriter`, `Logger`, `StreamChannelRef`, and `otel?: TelemetryOptions` from `InitOptions`.
  - **Python** (`sdk/packages/python/iii/src/iii/__init__.py`): drop `ChannelReader`, `ChannelWriter`, `Logger`, `StreamChannelRef`, `OtelConfig`, `TelemetryOptions`.
  - **Rust** (`sdk/packages/rust/iii/src/lib.rs:114-127`): drop the entire `telemetry` module re-export plus `ChannelDirection`, `ChannelItem`, `ChannelReader`, `ChannelWriter`, `StreamChannelRef`, `extract_channel_refs`, `is_channel_ref`, `Logger`, `OtelConfig`, `ReconnectionConfig`. Also remove `register_function_with` (`iii.rs:951`) — previously flagged for removal.
  - **Browser** (`sdk/packages/node/iii-browser/src/index.ts`): drop `ChannelReader`, `ChannelWriter`, and `TelemetryOptions` from `InitOptions`.

- **Website `/docs/*` link rewrites or Mintlify redirects.** One-time deploy task; not docs-content work but blocks public navigation alignment.

## Recommended next actions (prioritized)

1. ~~**Decide on top-level homeless commands** (`iii cloud`, `iii update`, `iii sandbox`).~~ **Done — `iii cloud` and `iii update` stubbed in `using-iii/deployment.mdx` and `using-iii/cli.mdx`; `iii sandbox` routes to Worker Docs (subcommand being removed).**
2. **Confirm with SDK authors whether browser SDK should export `IIIReconnectionConfig` / `RegisterFunctionFormat`.** Docs currently strip them; if export is intended, re-add Types entries on `browser-sdk.mdx`.
3. ~~**Decide adapter status** (genuinely deprecated vs not).~~ **Done — rule confirmed; engine cleanup tracked in hanging pieces.**
4. **Audit and reconcile SDK forbidden-surface exports.** Either tighten the SDK packages or relax `sdks.md`.
5. ~~**Add Rust SDK methods** (`set_headers`, `set_metadata`, `set_otel_config`, `get_connection_state`, `register_function_with`) to `rust-sdk.mdx`, OR mark them internal.~~ **`register_function_with` queued for removal; remaining four tracked in hanging pieces (SDK-author confirmation needed).**
6. ~~**Add `get_connection_state`** to `python-sdk.mdx`.~~ **Tracked in hanging pieces (SDK-author confirmation needed).**
7. ~~**Normalize `iii-config.yaml` → `config.yaml`** in `engine/README.md` and per-worker README files.~~ **Tracked in hanging pieces (engine team hand-off).**
8. ~~**Disambiguate `iii console` vs `iii-console`** in `using-iii/console.mdx` Launch stub.~~ **Done — `iii-console` is internal/local-binary only; docs use `iii console` exclusively.**
9. ~~**Configure website→docs URL redirects** (deploy task).~~ **Done — docs paths switched to match website (`/install`, `/quickstart` at root).**
