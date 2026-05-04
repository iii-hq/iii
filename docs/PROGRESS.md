# Migration Progress

Tracking the analysis of every `.mdx` file from `iii-mono/docs/` against the new `ideal-docs` structure.

**Skip:** files under `0-10-0/` (legacy).

**Ground rules:**
- Specific iii workers (state, queue, stream, cron, pubsub, http, observability, bridge, exec, worker-manager, etc.) will have their own dedicated **Worker Docs** outside this project. Worker-specific content must be recommended for **Move to Worker Docs** — but we never actually move it; that's someone else's exercise. Just note it in the decision log.
- **Adapters are deprecated.** The default recommendation for adapter content is remove, or salvage the concept into a future "Adapter Pattern" page under a new "Patterns" section if the framing is broadly useful.
- **Config files:** the engine config is `config.yaml` (not `iii-config.yaml`). Worker-level config is `iii.worker.yaml`. Reference accordingly per scope.
- **Migrated content is minimal:** when moving content into an ideal-docs page, write only the section title plus at most one sentence describing what the section *should* contain. Do not paste original prose, tables, or code blocks. The point is to mark the slot, not to author the page.
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
- [ ] api-reference/sdk-python.mdx
- [ ] api-reference/sdk-rust.mdx

### architecture/
- [ ] architecture/index.mdx
- [ ] architecture/channels.mdx
- [ ] architecture/engine.mdx
- [ ] architecture/external-workers.mdx
- [ ] architecture/queues.mdx
- [ ] architecture/trigger-types.mdx
- [ ] architecture/workers.mdx

### changelog/
- [ ] changelog.mdx
- [ ] changelog/0-11-0/everything-is-a-worker.mdx
- [ ] changelog/0-11-0/migrated-examples.mdx
- [ ] changelog/0-11-0/migrating-from-motia-js.mdx
- [ ] changelog/0-11-0/migrating-from-motia-py.mdx
- [ ] changelog/0-11-0/remove-dict-form-from-register-function.mdx

### console/
- [ ] console/index.mdx

### examples/
- [ ] examples/conditions.mdx
- [ ] examples/cron.mdx
- [ ] examples/hello-world.mdx
- [ ] examples/multi-trigger.mdx
- [ ] examples/observability.mdx
- [ ] examples/state-management.mdx
- [ ] examples/todo-app.mdx

### how-to/
- [ ] how-to/configure-engine.mdx
- [ ] how-to/create-custom-trigger-type.mdx
- [ ] how-to/create-ephemeral-worker.mdx
- [ ] how-to/dead-letter-queues.mdx
- [ ] how-to/define-request-response-formats.mdx
- [ ] how-to/developing-sandbox-workers.mdx
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

- **Source-doc gap:** iii-mono `api-reference/sdk-browser.mdx` omits `IIIReconnectionConfig` and `RegisterFunctionFormat` types — both exist in browser SDK source. Stubs included here; flag for the source author.
