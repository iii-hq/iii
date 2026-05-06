# WORK — Worker Docs

Remaining Worker Docs work. Sourced from
[`PROGRESS.md`](../PROGRESS.md) and [`PROGRESS-repo-checks.md`](../PROGRESS-repo-checks.md).

**Important:** Worker Docs is a separate documentation surface from ideal-docs (the iii
docs). The ideal-docs migration **flagged** content for Worker Docs but **never moved
it** — that exercise is left to the Worker Docs authors. This file catalogs every flag.

Project rule: [`project-rules/workers.md`](../project-rules/workers.md).

## Worker registry (as of audit 2026-05-06)

13 workers registered in the engine. Each needs its own Worker Docs entry.

| Worker | Source | Mandatory | Default |
|---|---|---|---|
| `iii-engine-functions` | `engine/src/workers/engine_fn/mod.rs` | yes | yes |
| `iii-observability` | `engine/src/workers/observability/mod.rs:598` | yes | yes |
| `iii-telemetry` | `engine/src/workers/telemetry/mod.rs` | yes | yes |
| `iii-worker-manager` | `engine/src/workers/worker/mod.rs` | yes | yes |
| `iii-http` | `engine/src/workers/rest_api/api_core.rs:462` | no | yes |
| `iii-stream` | `engine/src/workers/stream/stream.rs:829` | no | yes |
| `iii-state` | `engine/src/workers/state/state.rs:409` | no | yes |
| `iii-cron` | `engine/src/workers/cron/cron.rs:169` | no | yes |
| `iii-queue` | `engine/src/workers/queue/queue.rs:803` | no | yes |
| `iii-pubsub` | `engine/src/workers/pubsub/pubsub.rs:193` | no | yes |
| `iii-exec` | `engine/src/workers/shell/worker.rs:68` | no | no |
| `iii-bridge` | `engine/src/workers/bridge_client/...` | no | no |
| `iii-http-functions` | `engine/src/workers/http_functions/...` | no | no |

Plus a workers hub page (`workers/index.mdx` flagged in original PROGRESS.md).

## Per-worker checklists

Every worker needs (per `project-rules/workers.md`):
- Config schema (the `config:` block under each worker entry in `config.yaml`).
- Functions the worker registers (e.g., `state::set`, `iii::durable::publish`).
- Trigger types the worker provides (e.g., `http`, `cron`, `state`).
- Worker-internal protocol details / error codes.
- Per-worker README cleanup: normalize `iii-config.yaml` → `config.yaml` per
  `project-rules/config.md` (engine-side hand-off; see [`WORK.engine.md`](./WORK.engine.md)).

Below: known content flagged from the source-docs migration plus notes from the
repo-checks audit.

### `iii-engine-functions`
- Functions: `engine::channels::create`, `engine::functions::list`, `engine::workers::list`,
  `engine::workers::register`, `engine::triggers::list`, `engine::trigger-types::list`.
- Trigger types: `engine::functions-available`, `engine::workers-available`.
- ideal-docs callouts already point readers here for introspection (see
  `understanding-iii/engine.mdx`, `sdk-reference/engine-sdk.mdx`,
  `using-iii/console.mdx`, `using-iii/workers.mdx`).
- [ ] Engineering review: should the worker rename or stay as iii-engine-functions?
  ideal-docs surfaces it as "Engine SDK" — see PROGRESS-repo-checks hanging piece.

### `iii-observability`
- Functions: `engine::log::{info,warn,error,debug,trace}`,
  `engine::baggage::{get,set,get_all}`, `engine::traces::{list,tree,clear}`,
  `engine::metrics::list`, `engine::logs::{list,clear}`, `engine::sampling::rules`,
  `engine::health::check`, `engine::alerts::{list,evaluate}`, `engine::rollups::list`.
- Trigger types: `log`.
- Surface from prior PROGRESS.md flags:
  - Logger methods (debug/info/warn/error).
  - OpenTelemetry init / custom spans / worker metrics / log emission and subscription.
  - OTel data-flow diagram (from `advanced/telemetry.mdx`).
  - iii-observability worker config block.
- Surface from PROGRESS-repo-checks introspection callouts:
  - Streaming traces / logs / events as triggers (the reactive primitive for live-event
    consumption — both consumer side: subscribe to log events; and producer side: emit
    logs that trigger downstream functions).
- [ ] Engineering review: the `engine::` prefix on these functions is misleading
  (they're worker-owned, not engine-internal). Decide whether to rename. See
  `PROGRESS-repo-checks.md` hanging piece.
- [ ] Disambiguate from iii-telemetry per `project-rules/workers.md` (different env
  vars: `OTEL_ENABLED` for iii-observability vs `III_TELEMETRY_ENABLED` for iii-telemetry).

### `iii-telemetry`
- Distinct from iii-observability. Anonymous-usage analytics (Amplitude), heartbeat,
  device-ID management.
- Governed by `III_TELEMETRY_ENABLED`.
- [ ] Worker Docs page is new (PROGRESS.md treated observability as the umbrella;
  reality is two workers).

### `iii-worker-manager`
- Channels: concept, lifecycle, writer/reader/refs, examples, channel-related types
  (`Channel`, `ChannelReader`, `ChannelWriter`, `ChannelDirection`, `StreamChannelRef`).
- Worker lifecycle (connect/disconnect, channels), connection mgmt.
- Worker authentication, RBAC, per-session namespacing, allow/deny lists,
  trigger-registration RBAC (from `how-to/worker-rbac.mdx`).
- iii.lock-related worker-side equivalents if any (the user-facing iii.lock surface
  stays in `using-iii/workers.mdx`).
- ideal-docs callouts on each SDK page already point readers here for channels.
- [ ] Engineering review: should `engine::channels::create` registration move from
  iii-engine-functions to iii-worker-manager? See `PROGRESS-repo-checks.md` hanging piece.

### `iii-http`
- Config schema (host, port, default_timeout, concurrency_request_limit, cors).
- HTTP trigger type (`http`).
- HTTP `http()` wrapper, file-download, SSE.
- HTTP middleware (engine-level pre-handler functions; from `how-to/use-http-middleware.mdx`).
- HTTP endpoint exposure (from `how-to/expose-http-endpoint.mdx`).
- [ ] Engineering review: `iii-http` vs `iii-http-functions` naming clash; consider
  renaming. See `PROGRESS-repo-checks.md` hanging piece.

### `iii-http-functions`
- Outbound HTTP-invocation surface (calling external HTTP services from inside iii
  functions).
- Distinct from `iii-http` (the inbound REST API server).
- [ ] Engineering review: name is confusingly close to `iii-http`; consider rename
  (`iii-http-client`, `iii-http-outbound`).

### `iii-stream`
- Stream `set` / `update` / `delete` change events (from `how-to/stream-realtime-data.mdx`).
- Stream trigger types: `stream:join`, `stream:leave`, `stream`.
- Stream Protocol (Join / Leave / Sync / Create / Update / Delete) — from
  `advanced/protocol.mdx` Stream Protocol section.
- iii-stream config schema, including auth_function field.

### `iii-state`
- Functions: `state::get`, `state::set`, `state::delete` and scopes (from
  `how-to/manage-state.mdx`).
- Trigger types: `state:updated`, `state:deleted` (state-changed triggers from
  `how-to/react-to-state-changes.mdx`).
- State worker config schema.
- Whole-page sources: `examples/state-management.mdx`.

### `iii-cron`
- `cron` trigger type and expression syntax (from `how-to/schedule-cron-task.mdx`).
- Per `examples/cron.mdx` — scheduled-job patterns (whole page).
- iii-cron config schema.

### `iii-queue`
- Named-queue mechanics (from `how-to/use-named-queues.mdx`).
- Topic queues, `durable:subscriber` trigger, `iii::durable::publish` (from
  `how-to/use-topic-queues.mdx`).
- DLQ inspection and redrive (`iii::queue::redrive`) (from `how-to/dead-letter-queues.mdx`).
- Queue models, RabbitMQ topology, retry, DLQ (from `architecture/queues.mdx`).
- iii-queue config schema.

### `iii-pubsub`
- `pubsub:subscriber` / `subscribe` trigger type.
- iii-pubsub config schema.

### `iii-bridge`
- Remote worker bridging, forwarding functions across engines.
- Confirmed as a Worker Docs target during the audit.

### `iii-exec` (or sandbox worker — name TBD)
- Sandbox runtime: VM-based isolated worker execution.
- Sandbox functions per the `iii sandbox` removal: `sandbox::run`, `sandbox::create`,
  `sandbox::exec`, `sandbox::list`, `sandbox::stop`, `sandbox::upload`, `sandbox::download`.
- Sandbox SDK page (from `sdk-reference/sandbox.mdx` — entirely Worker-Docs scope).
- Sandbox-worker authoring (from `how-to/developing-sandbox-workers.mdx`) — note this
  is worker-authoring content and may go in a separate worker-author guide.
- See [`CLARIFICATION_NEED.sandbox-worker.md`](./CLARIFICATION_NEED.sandbox-worker.md)
  for the open question on which worker hosts the sandbox functions.

## Cross-cutting Worker Docs items

### Workers hub page
- [ ] Author the workers hub page (`workers/index.mdx` flagged in PROGRESS.md). Should
  enumerate the 13 workers and their primary purposes.

### Per-worker config schemas
- [ ] Each worker's `config.yaml` block needs a config-reference section. Pending the
  auto-gen tooling design (see [`WORK.auto-gen.md`](./WORK.auto-gen.md)).
- Per `config.md` ground rule: engine config is `config.yaml`; worker-level config is
  `iii.worker.yaml`.

### Streaming traces / logs / events as triggers
- Flagged by the user as critical for humans and agent skills.
- [ ] iii-observability Worker Docs should cover this pattern explicitly:
  - Consumer side: subscribe to log events via the `log` trigger.
  - Producer side: emit logs that trigger downstream functions.
  - Cross-link from iii-engine-functions Worker Docs (live-discovery patterns).

### Worker-rbac
- [ ] iii-worker-manager Worker Docs should cover (from `how-to/worker-rbac.mdx`):
  - Auth functions, middleware.
  - Per-session namespacing.
  - Allow/deny lists.
  - Trigger-registration RBAC.

### Adapter content
- Per `project-rules/general.md`: adapters are deprecated. Worker Docs should NOT
  document adapter blocks. Engine-side cleanup of adapter code is in
  [`WORK.engine.md`](./WORK.engine.md).

## Hanging items from PROGRESS.md to confirm in Worker Docs

These items were flagged for Worker Docs in the original migration. Confirming they all
have a home in the Worker Docs target:

- [x] `architecture/channels.mdx` → iii-worker-manager (channels)
- [x] `architecture/queues.mdx` → iii-queue
- [x] `architecture/external-workers.mdx` → distributed (no single worker home;
  ideal-docs absorbed the user-facing parts).
- [x] `architecture/trigger-types.mdx` per-trigger-type sections → respective workers.
- [ ] `architecture/workers.mdx` "Built-in workers" enumeration table → flagged for
  Worker Docs but better suited to the workers hub page.
- [x] `architecture/engine.mdx` ports table + per-worker YAML snippets → respective
  workers.
- [x] `advanced/telemetry.mdx` worker config + OTel data-flow diagram → iii-observability.
- [x] `advanced/protocol.mdx` Stream Protocol section → iii-stream.
- [x] `advanced/architecture.mdx` per-worker architecture sections (HTTP/Streams/Queue/Cron)
  → respective workers.
- [x] `examples/cron.mdx`, `examples/observability.mdx`, `examples/state-management.mdx`
  → respective workers.
- [x] `sdk-reference/sandbox.mdx` → sandbox Worker Docs.

## Worker authoring (out of scope — different project)

Per `project-rules/workers.md`: implementing engine traits, building a custom worker
package, designing per-language worker scaffolds — all worker-authoring content
belongs OUTSIDE iii docs (Worker Docs authoring guides, not user-facing iii docs).

This is mentioned here because PROGRESS.md flagged some "developing sandbox workers"
content; that goes in a worker-authoring guide, not in user-facing Worker Docs.

## Notes / dependencies

- Worker Docs is a separate doc deployment from ideal-docs.
- Each Worker Docs entry should align with the per-worker README cleanup happening
  engine-side (see [`WORK.engine.md`](./WORK.engine.md) — `iii-config.yaml` →
  `config.yaml` normalization).
- Engineering naming reviews ([`WORK.engine.md`](./WORK.engine.md) §4) may rename
  workers; Worker Docs page filenames should follow the resolved names.
- The auto-gen config reference ([`WORK.auto-gen.md`](./WORK.auto-gen.md)) may
  generate per-worker config schemas; coordinate so Worker Docs authors don't
  hand-write content the generator will overwrite.
