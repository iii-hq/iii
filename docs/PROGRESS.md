# Migration Progress

Tracking the analysis of every `.mdx` file from `iii-mono/docs/` against the new `ideal-docs` structure.

**Skip:** files under `0-10-0/` (legacy).

**Ground rules:**
- Specific iii workers (state, queue, stream, cron, pubsub, http, observability, bridge, exec, worker-manager, etc.) will have their own dedicated **Worker Docs** outside this project. Worker-specific content must be recommended for **Move to Worker Docs** — but we never actually move it; that's someone else's exercise. Just note it in the decision log.
- **Adapters are deprecated.** The default recommendation for adapter content is remove, or salvage the concept into a future "Adapter Pattern" page under a new "Patterns" section if the framing is broadly useful.
- **Config files:** the engine config is `config.yaml` (not `iii-config.yaml`). Worker-level config is `iii.worker.yaml`. Reference accordingly per scope.
- **Migrated content is minimal:** when moving content into an ideal-docs page, write only the section title plus at most one sentence describing what the section *should* contain. Do not paste original prose, tables, or code blocks. The point is to mark the slot, not to author the page.
- **Logger and telemetry surfaces belong with the iii-observability worker.** Strip them from SDK reference pages and leave a callout pointing readers to the iii-observability worker docs. Drop telemetry-related SDK types (`OtelConfig`, OTel-specific `ReconnectionConfig`, `TelemetryOptions`, etc.) from SDK type lists.
- **Channels belong to iii-worker-manager.** The whole channel surface (concept, lifecycle, writer/reader/refs, examples) belongs in the iii-worker-manager Worker Docs. Strip channel-related types (`Channel`, `ChannelReader`, `ChannelWriter`, `ChannelDirection`, `StreamChannelRef`) from SDK reference pages and add a callout pointing readers to iii-worker-manager.
- **No "external vs built-in" worker distinction.** A worker is a worker. Don't introduce conceptual splits between SDK-driven processes and engine-bundled workers in ideal-docs. If source content draws that distinction, drop or flatten it.
- **Auto-reconnect / re-registration is SDK behavior, not a worker concept.** All current SDKs implement it (Node, Python, Rust, browser). Document it on the per-SDK reference pages (already covered by each page's "Connection lifecycle" stub), not on `understanding-iii/workers.mdx`.
- **`expanding-iii/` scope:** "Expanding iii" means expanding an iii *system* with more workers and functionality (deploying / wiring up / integrating additional workers). It is **not** about adding code to the iii engine itself. All iii expansion is worker expansion. Content about *authoring* a worker (implementing engine traits, building a custom worker package) does not belong in expanding-iii.

## Status legend

- [ ] pending
- [x] decided
- [~] decided, but with hanging pieces still needing a home (see Hanging Pieces section)

## File list

### advanced/
- [x] advanced/adapters.mdx
- [x] advanced/architecture.mdx
- [x] advanced/custom-modules.mdx
- [x] advanced/deployment.mdx
- [x] advanced/protocol.mdx
- [x] advanced/telemetry.mdx

### api-reference/
- [x] api-reference/disable-telemetry.mdx
- [x] api-reference/sandbox.mdx
- [x] api-reference/sdk-browser.mdx
- [x] api-reference/sdk-node.mdx
- [x] api-reference/sdk-python.mdx
- [x] api-reference/sdk-rust.mdx

### architecture/
- [x] architecture/index.mdx
- [x] architecture/channels.mdx
- [x] architecture/engine.mdx
- [x] architecture/external-workers.mdx
- [x] architecture/queues.mdx
- [x] architecture/trigger-types.mdx
- [x] architecture/workers.mdx

### changelog/
- [x] changelog.mdx
- [x] changelog/0-11-0/everything-is-a-worker.mdx
- [x] changelog/0-11-0/migrated-examples.mdx
- [x] changelog/0-11-0/migrating-from-motia-js.mdx
- [x] changelog/0-11-0/migrating-from-motia-py.mdx
- [x] changelog/0-11-0/remove-dict-form-from-register-function.mdx

### console/
- [x] console/index.mdx

### examples/
- [x] examples/conditions.mdx
- [x] examples/cron.mdx
- [x] examples/hello-world.mdx
- [x] examples/multi-trigger.mdx
- [x] examples/observability.mdx
- [x] examples/state-management.mdx
- [x] examples/todo-app.mdx

### how-to/
- [x] how-to/configure-engine.mdx
- [x] how-to/create-custom-trigger-type.mdx
- [x] how-to/create-ephemeral-worker.mdx
- [x] how-to/dead-letter-queues.mdx
- [x] how-to/define-request-response-formats.mdx
- [x] how-to/developing-sandbox-workers.mdx
- [ ] how-to/expose-http-endpoint.mdx
- [ ] how-to/manage-state.mdx
- [ ] how-to/managing-container-workers.mdx
- [ ] how-to/observability-and-logs.mdx
- [ ] how-to/react-to-state-changes.mdx
- [ ] how-to/reproduce-worker-installs.mdx
- [ ] how-to/schedule-cron-task.mdx
- [ ] how-to/stream-realtime-data.mdx
- [ ] how-to/trigger-actions.mdx
- [ ] how-to/trigger-functions-from-cli.mdx
- [ ] how-to/use-channels.mdx
- [ ] how-to/use-console.mdx
- [ ] how-to/use-functions-and-triggers.mdx
- [ ] how-to/use-http-middleware.mdx
- [ ] how-to/use-iii-in-the-browser.mdx
- [ ] how-to/use-named-queues.mdx
- [ ] how-to/use-topic-queues.mdx
- [ ] how-to/use-trigger-conditions.mdx
- [ ] how-to/worker-rbac.mdx

### primitives-and-concepts/
- [ ] primitives-and-concepts/discovery.mdx
- [ ] primitives-and-concepts/functions-triggers-workers.mdx

### root pages
- [ ] index.mdx
- [ ] install.mdx
- [ ] quickstart.mdx

### tutorials/
- [ ] tutorials/index.mdx

### workers/
- [ ] workers/index.mdx
- [ ] workers/iii-bridge.mdx
- [ ] workers/iii-cron.mdx
- [ ] workers/iii-exec.mdx
- [ ] workers/iii-http.mdx
- [ ] workers/iii-observability.mdx
- [ ] workers/iii-pubsub.mdx
- [ ] workers/iii-queue.mdx
- [ ] workers/iii-state.mdx
- [ ] workers/iii-stream.mdx
- [ ] workers/iii-worker-manager.mdx
- [ ] workers/managed-worker-lockfile.mdx

## Decisions log

### advanced/adapters.mdx — Remove
- Adapters are deprecated; remove the page in its current form.
- No new ideal-docs page needed. No "Choosing an Adapter" matrix.
- Per-worker adapter sections (Queue/State/Stream/Cron/PubSub) are worker-specific — flagged for Worker Docs, not migrated here.

### how-to/developing-sandbox-workers.mdx — Move to Worker Docs (iii-sandbox / iii-worker-manager)
- Sandbox is a worker; worker authoring belongs outside ideal-docs.
- No stubs.

### how-to/define-request-response-formats.mdx — Distribute and drop
- Stub added to `using-iii/functions.mdx`: "Define request and response formats".
- "Discover from CLI" piece already covered (`using-iii/cli.mdx`, `api-reference/engine-sdk.mdx` discovery functions).
- Drops: code blocks, "Why this matters" framing, auto-extraction vs explicit comparison (lives on SDK pages), "Next steps" card.

### how-to/dead-letter-queues.mdx — Move to Worker Docs (iii-queue)
- DLQ inspection and redrive (`iii::queue::redrive`) belong with iii-queue.

### how-to/create-ephemeral-worker.mdx — Distribute and drop
- Stub added to `using-iii/deployment.mdx`: "Run an ephemeral worker" — one-shot SDK process pattern for serverless containers / k8s Jobs.

### how-to/create-custom-trigger-type.mdx — Drop entirely
- Worker-author flow that pairs with the SDK's `registerTriggerType` / `unregisterTriggerType` methods, both already marked "Consider removing" on each SDK page.
- If trigger-type registration becomes worker-author / worker-internal, this how-to belongs outside ideal-docs.
- No stubs.

### how-to/configure-engine.mdx — Drop, restore as generated content (post-outline)
- Generic engine-config concepts already covered: `using-iii/engine.mdx` (Configuration file structure, Environment variable expansion, Built-in defaults), `using-iii/cli.mdx` (CLI arguments).
- Per-worker config schemas (HTTP, Stream, State, Queue, Cron, Observability, etc.) → flagged for Worker Docs; with "everything is a worker," much of what looked like engine config is really per-worker config.
- See Hanging pieces for the auto-generation restoration plan.

### examples/todo-app.mdx — New how-to
- Created `how-to/build-a-realtime-todo-app.mdx` with section stubs lifted from the source: Engine configuration, Backend (Worker setup, Auth function/RBAC, Stream wrapper, Functions), Frontend (Connection, Real-time hook), Key concepts.
- Wired into nav under the How-to group.
- Verified app is genuinely reactive (single browser WebSocket, iii-stream change events, per-session RBAC namespacing).

### examples/state-management.mdx — Move to Worker Docs (iii-state)
- State is a worker.

### examples/observability.mdx — Move to Worker Docs (iii-observability)
- Observability is a worker; per existing ground rule, logger/telemetry surfaces and examples belong with iii-observability.

### examples/multi-trigger.mdx — Drop entirely
- Concept already covered: `understanding-iii/triggers.mdx` "Multiple triggers per function" + `using-iii/triggers.mdx` "Bind multiple triggers to one function".

### examples/hello-world.mdx — Drop entirely
- Overlaps with `getting-started/quickstart.mdx` (the canonical first-build) and `tutorials/reactive-crud/*`.
- No stubs.

### examples/cron.mdx — Move to Worker Docs (iii-cron)
- Cron is a worker. Entire page (cron trigger type, scheduled-job patterns) → iii-cron Worker Docs.
- No stubs in ideal-docs.

### examples/conditions.mdx — Drop entirely
- Concept already covered: `understanding-iii/triggers.mdx` "Trigger conditions" + `using-iii/triggers.mdx` "Gate a trigger with a condition".
- Code-heavy single-purpose example; doesn't fit ideal-docs structure.

### console/index.mdx — Heavy stubs on using-iii/console
- Heavy stubs added on `using-iii/console.mdx` covering each console UI section: Launch, Workers, Functions, Triggers, States, Streams, Queues, Traces, Logs, Flow, Configuration.
- Drops: screenshots, code blocks, all info/how-to cross-reference callouts, console-vs-console-output disambiguation, system Mermaid diagram (covered by `understanding-iii/index.mdx`).
- No Worker Docs flag (the console is its own surface, not a worker).

### changelog.mdx + changelog/0-11-0/*.mdx — Stub only
- Created `changelog.mdx` at the root with a one-line description; no per-version content.
- All 0-11-0 detail pages skipped. Per-version migration content (Motia migration guides, "everything is a worker" rename, `registerFunction` signature change, etc.) is not migrated to ideal-docs.
- Wired into nav as a top-level page next to `index`.

### architecture/workers.mdx — Drop (mostly)
- No new stubs added (per user decision).
- Built-in workers enumeration table → flagged for Worker Docs.
- Drops: "external vs built-in vs managed" types table (no meaningful distinction), adapter YAML, `iii-config.yaml` reference, Custom Workers callout (target page being dropped), terminology note, all code snippets.

### architecture/trigger-types.mdx — Distribute and drop
- Stubs added to `understanding-iii/triggers.mdx`: Trigger components, Trigger pipeline, Trigger lifecycle, Multiple triggers per function, Trigger conditions.
- Stubs added to `using-iii/triggers.mdx`: Register a trigger, Bind multiple triggers to one function, Gate a trigger with a condition, Unregister a trigger.
- Per-trigger-type sections (http, durable:subscriber/queue, cron, log, stream:join/leave) → flagged for their respective workers (iii-http, iii-queue, iii-cron, iii-observability, iii-stream).
- HTTP `http()` wrapper, file-download, SSE → flagged for iii-http.
- Drops: code blocks, Best Practices accordion, Custom Trigger Types Rust snippet, "Next Steps" card, Trigger Type Comparison matrix (per-worker behaviors).

### architecture/queues.mdx — Move to Worker Docs (iii-queue)
- Queues are a worker. Whole page (models, RabbitMQ topology, retry, DLQ, `iii::queue::redrive`, adapter comparison) → Worker Docs.
- No stubs in ideal-docs. No new pages.

### architecture/external-workers.mdx — Distribute and drop
- New ground rules added: no external/built-in worker distinction; auto-reconnect is SDK behavior (already on each SDK's "Connection lifecycle" stub).
- Stubs added to `understanding-iii/workers.mdx`: Worker lifecycle states, Worker isolation.
- `engine::workers::register` added to the existing engine-discovery-functions stub on `api-reference/engine-sdk.mdx`.
- Drops: SDK package table, code snippets, InitOptions table (covered on SDK pages), Reconnection Config field detail (covered by SDK type entries), Python snake_case note, both diagrams, "See also" card.

### architecture/engine.mdx — Distribute and drop
- Stubs added to `understanding-iii/engine.mdx`: Engine responsibilities, Worker disconnect cleanup, Config hot-reload, Architecture-agnostic routing.
- Stub added to `api-reference/engine-sdk.mdx`: Engine discovery functions (`engine::functions::list`, `engine::workers::list`, `engine::workers-available`).
- Ports table and per-worker YAML snippets — flagged for Worker Docs (each port/config belongs to its built-in worker).
- Drops: duplicate Responsibilities table, `iii-config.yaml` refs (should be `config.yaml`), adapter YAML, version-skew advisory, reload log-line examples, "See also" linkrot card.

### architecture/channels.mdx — Move to Worker Docs (iii-worker-manager)
- Channels belong to iii-worker-manager. Entire page (concept, lifecycle, writer/reader/refs, examples) → iii-worker-manager Worker Docs.
- Stripped channel types from SDK pages (`StreamChannelRef` was the only one still present, in browser-sdk and node-sdk; removed from both).
- Added a channels callout to all four SDK pages alongside the observability callout, pointing at iii-worker-manager.
- Ground rule corrected.

### architecture/index.mdx — New pages in understanding-iii
- Created `understanding-iii/index.mdx` (stubs: "The three primitives", "System overview").
- Created `understanding-iii/functions.mdx` (stubs: "What a function is", "Function identifiers").
- Created `understanding-iii/triggers.mdx` (stubs: "What a trigger is", "Trigger types").
- Wired all three into `docs.json` under the Understanding iii group.
- Dropped the contradictory "four components" line and per-built-in-worker module enumeration.

### api-reference/sdk-rust.mdx — Heavy stubs on rust-sdk
- Source verified against `sdk/packages/rust/iii/src/`.
- Logger + Telemetry stripped; observability callout in place.
- Methods trimmed to the cross-SDK pattern: register_worker (with InitOptions), register_function, register_trigger, trigger, shutdown, shutdown_async, register_trigger_type / unregister_trigger_type ("Consider removing"). Dropped: `set_headers`, `register_function_with`, `register_service`, `create_channel`, `get_connection_state`.
- Types: kept Rust-relevant ones (IIIError, IIIConnectionState, HttpAuthConfig, HttpInvocationConfig, HttpMethod, TriggerAction, TriggerRequest, Trigger, FunctionRef, FunctionInfo, TriggerInfo, WorkerInfo, WorkerMetadata, RegisterFunctionMessage, RegisterServiceMessage). Dropped OtelConfig + OTel `ReconnectionConfig` per observability rule.
- Cross-SDK mismatches recorded in Hanging pieces.

### api-reference/sdk-python.mdx — Heavy stubs on python-sdk
- Source verified: Python SDK only has one `ReconnectionConfig` (engine WS); no separate OTel reconnection config.
- Logger and Telemetry surfaces stripped from all SDK pages with a callout pointing to iii-observability worker docs (per new ground rule).
- Heavy stubs added on `api-reference/python-sdk.mdx`: Installation, Initialization (with `worker` convention), Connection lifecycle, Methods (register_worker w/ InitOptions, register_function, register_trigger, trigger + trigger_async, shutdown + shutdown_async, register_trigger_type / unregister_trigger_type marked "Consider removing"), Types (ReconnectionConfig, HttpInvocationConfig, RegisterFunctionFormat, RegisterServiceInput, RegisterTriggerInput, RegisterTriggerTypeInput, TriggerActionEnqueue, TriggerActionVoid, TriggerRequest, TriggerHandler, IStream).

### api-reference/sdk-node.mdx — Heavy stubs on node-sdk
- Source verified: `IIIReconnectionConfig` (engine WS) and `ReconnectionConfig` (OTel telemetry-system) are both real and distinct in node SDK; `OtelConfig`, `HttpAuthConfig`, `HttpInvocationConfig` all exist.
- Heavy stubs added on `api-reference/node-sdk.mdx`: Installation, Initialization, Connection lifecycle, all 9 methods, Subpath exports, Logger (debug/info/warn/error), Telemetry suite, full type list.

### api-reference/sdk-browser.mdx — Heavy stubs on browser-sdk
- Verified against `sdk/packages/node/iii-browser` source: browser SDK does **not** ship OpenTelemetry. Removed the 5 telemetry stubs incorrectly added during the file 6 pass (Telemetry, Custom spans, Worker metrics, Log emission and subscription, Telemetry utilities).
- Heavy stubs added on `api-reference/browser-sdk.mdx`: Installation, Initialization, Connection lifecycle, all 9 methods, Subpath exports, and full type list.
- **Source-doc inaccuracy noted (hanging piece):** iii-mono `sdk-browser.mdx` omits `IIIReconnectionConfig` and `RegisterFunctionFormat` types even though both exist in the browser SDK source.

### api-reference/sandbox.mdx — Move to Worker Docs (sandbox)
- Entire page is sandbox-worker-specific (SDK calls, engine config, images, errors, `iii sandbox` CLI subcommands).
- Flagged for Worker Docs. No stubs in ideal-docs.

### api-reference/disable-telemetry.mdx — Already mapped
- 1:1 with existing `how-to/disable-telemetry.mdx`. No changes needed.
- When the page is authored, distinguish `III_TELEMETRY_ENABLED` (anonymous usage) from `OTEL_ENABLED` (observability instrumentation).

### advanced/telemetry.mdx — Distribute and drop
- Stubs added to each client SDK page (node, python, rust, browser): Telemetry, Custom spans, Worker metrics, Log emission and subscription, Telemetry utilities.
- Stub added to `api-reference/engine-sdk.mdx`: Engine-collected invocation metrics.
- iii-observability worker config + OTel data-flow diagram → flagged for Worker Docs (iii-observability).
- Cross-SDK Comparison matrix dropped.

### advanced/protocol.mdx — Distribute and drop
- Stubs added to `api-reference/engine-sdk.mdx`: Message types, Connection flow, Invocation lifecycle, Register function/trigger/invoke/result messages.
- Stream Protocol section (Join/Leave/Sync/Create/Update/Delete) — flagged for iii-stream Worker Docs.

### advanced/deployment.mdx — Distribute and drop
- Install section drops (covered by `getting-started/install.mdx`).
- Stubs added: `using-iii/cli.mdx` (CLI arguments), `using-iii/engine.mdx` (Configuration file structure, Environment variable expansion, Built-in defaults), `understanding-iii/engine.mdx` (Startup flow).
- New page created: `using-iii/deployment.mdx` with stubs for Docker, reverse proxy, production hardening, multi-instance, health checks. Wired into nav.
- Worker-specific bits (iii-observability config, per-worker adapter snippets) — flagged for Worker Docs (and adapter content drops anyway).

### advanced/custom-modules.mdx — Drop (mostly)
- Page is Rust-specific worker-authoring guide; drop all Rust + impl detail (traits, lifecycle code, factories, complete example, best practices snippets).
- Drop all adapter content (deprecated). Drop references to `iii-config.yaml`.
- Added high-level stubs to `expanding-iii/workers.mdx`: "How workers expand iii", "Configuring workers", "What a worker contributes".
- Note: the page should generally be dropped — worker-authoring belongs outside ideal-docs.

### advanced/architecture.mdx — Drop as source
- Page is a sprawling overview that mostly duplicates the per-area `architecture/*.mdx` files; absorb from those instead.
- Per-worker architecture sections (HTTP / Streams / Queue / Cron) — flagged for Worker Docs.
- "Custom Adapters" subsection — drop (deprecated).
- "Package Architecture" diagram — drop (redundant with SDK Connection Flow).
- "SDK Connection Flow" (lifecycle / heartbeat / reconnect / re-registration) — add stub section in each client SDK reference page (`api-reference/node-sdk.mdx`, `python-sdk.mdx`, `rust-sdk.mdx`, `browser-sdk.mdx`).

## Hanging pieces

(Content with no clear home — track here so we don't lose it.)

- **Restore auto-generated config reference (post-outline engineering task).** Pre-Mintlify, `how-to/configure-engine.mdx` was generated from a commented `iii-config.yaml` by a build-time parser + React component (commit `0f925fd2` in iii-mono, files: `docs/content/how-to/iii-config.yaml`, `docs/src/lib/config-parser.ts`, `docs/src/lib/config-toc.ts`, `docs/src/lib/components/ConfigReference.tsx`). Lost in the `feat: mintlify` migration (`79bdd94d`).
  - Goal: regenerate the configuration reference and surface it inside `using-iii/engine.mdx` (or wherever fits best after final structure).
  - Implementation sketch: (1) author a fresh commented `config.yaml` in iii-mono colocated with the engine — current naming (`name: iii-http`, etc.), no deprecated adapter sections; (2) write a generator that emits a Mintlify MDX fragment (pre-build script committing `using-iii/engine.config-reference.generated.mdx`, or a `_snippets/config-reference.mdx` consumed via `<Snippet file="..." />`); (3) transclude into the host page.
  - **Open question — much of this may belong to Worker Docs, not ideal-docs.** With "everything is a worker," what used to read as engine config is largely per-worker config (HTTP host/port/CORS → iii-http; stream auth_function → iii-stream; etc.). Decide during post-outline review whether the reference is one cross-cutting page in ideal-docs or split across each Worker Docs surface.
  - Don't attempt during this migration pass.

- **Source-doc gap:** iii-mono `api-reference/sdk-browser.mdx` omits `IIIReconnectionConfig` and `RegisterFunctionFormat` types — both exist in browser SDK source. Stubs included here; flag for the source author.

- **SDK type-list mismatches (Node vs Python, per source docs):**
  - **In Node, missing from Python:** `MessageType`, `FunctionRef`, `HttpAuthConfig`, `RegisterFunctionMessage`, `RegisterFunctionOptions`, `RegisterTriggerMessage`, `RegisterTriggerTypeMessage`, `RemoteFunctionHandler`, `StreamChannelRef`, `Trigger`, `TriggerTypeRef`, `DeleteResult`, `StreamSetResult`, `StreamUpdateResult`.
  - **In Python, missing from Node:** `TriggerActionEnqueue`, `TriggerActionVoid` (Python-specific trigger action variants).
  - **Naming difference:** Python `ReconnectionConfig` ≡ Node `IIIReconnectionConfig`. Python has only one (engine WS); Node has two (`IIIReconnectionConfig` for engine WS, `ReconnectionConfig` for OTel exporter), so the prefix disambiguates.
  - **Method difference:** Python exposes async variants (`trigger_async`, `shutdown_async`) that Node does not. Flagged in source by user with "This differs from node, is this okay?" — open question for SDK authors.
  - **Unverified — could be a doc gap or genuine SDK gap.** When authoring Rust SDK, verify each Node-side type against Python source code (not just docs) to determine which mismatches are real and which are doc omissions.

- **SDK type-list mismatches (Rust vs Node/Python, source-verified against `sdk/packages/rust/iii/src/`):**
  - **Rust does not expose `IIIReconnectionConfig` or any user-facing engine-WS reconnection config.** Reconnection happens internally (`IIIConnectionState::Reconnecting`) but is not configurable. Node and Python both expose this. Genuine SDK gap.
  - **Rust does not expose `RegisterFunctionFormat`** — uses `schemars::JsonSchema` derive directly on request/response types instead. Equivalent capability via different mechanism.
  - **Rust does not expose `MessageType`** — internal to the protocol module, not part of the public API. Node exports it.
  - **Rust adds (vs Node/Python):** `IIIError`, `IIIConnectionState`, `HttpMethod`, `ChannelDirection`, `FunctionInfo`, `TriggerInfo`, `WorkerInfo`, `WorkerMetadata`, `RegisterServiceMessage`, `register_function_with` (builder variant), `set_headers` method.
  - **Rust expresses Python's `TriggerActionEnqueue`/`TriggerActionVoid` as variants of a single `TriggerAction` enum** — equivalent at the type level; flagged here for cross-language alignment.
  - **Method differences:** Rust has `set_headers` (unique), `register_function_with` (Rust-only builder), and `shutdown_async` (matches Python; Node lacks).
  - **Why no `trigger_async` in Rust:** Rust's `trigger` is already declared `pub async fn trigger(...)` (verified in `sdk/packages/rust/iii/src/iii.rs:1132`), so a separate async variant isn't needed. `shutdown` has both sync and async forms because the sync drop path can be called from non-async contexts; `trigger` cannot. This is consistent with Rust idioms but a real cross-SDK shape difference.
