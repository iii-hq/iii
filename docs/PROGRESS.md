# Migration Progress

Tracking the analysis of every `.mdx` file from `iii-mono/docs/` against the new `ideal-docs` structure.

**Skip:** files under `0-10-0/` (legacy).

**Ground rules:** canonical rules now live in [`project-rules/`](./project-rules/) — split by domain (`general`, `workers`, `sdks`, `cli`, `console`, `config`). Refer to those when authoring or reviewing pages. Decisions in this log assume the rules in `project-rules/` apply.

## Migration summary

All 73 files from `iii-mono/docs/` analyzed and routed.

**Outcomes:**
- **Ideal-docs stubbed (~30):** distributed into existing or new pages as section-title + ≤1-sentence stubs.
- **Flagged for Worker Docs (~25):** worker-specific content noted for the relevant worker (iii-http, iii-queue, iii-state, iii-stream, iii-cron, iii-observability, iii-pubsub, iii-bridge, iii-exec, iii-worker-manager). Never moved — that exercise is left to the Worker Docs authors.
- **Dropped entirely (~18):** already absorbed elsewhere, code-heavy single-purpose examples, redundant hubs.

**New ideal-docs pages created during analysis:**
- `understanding-iii/index.mdx`, `understanding-iii/functions.mdx`, `understanding-iii/triggers.mdx`
- `using-iii/deployment.mdx`
- `how-to/build-a-realtime-todo-app.mdx`
- `changelog/index.mdx`

**Hanging pieces logged below:**
- Auto-generated config reference (post-outline engineering project; see commit `0f925fd2` for the deleted prior implementation).
- Auto-generated SDK reference docs need pre/post-append slots for narrative content.
- Source-doc gaps in iii-mono `sdk-browser.mdx` (missing types).

**Ground rules established and recorded above.**

## Process notes for next pass

Capturing what worked, what surprised us, and what to assume up front when reusing this file structure for a similar exercise (e.g., walking the rest of iii-mono — engine, SDKs, console — to align with `ideal-docs`).

### Workflow that worked
- **Per-file pause-and-decide cadence.** Skim section list → propose stubs / drops / Worker-Docs flags → wait for "yes" / refinement → apply → mark done + log decision. Going faster (proposing multiple files at once) lost detail.
- **Ground rules emerge iteratively.** Several major rules (channels belong to iii-worker-manager, no external/built-in distinction, `iii worker` CLI is iii-level, logger/telemetry belong to iii-observability) only became clear partway through and required revisiting earlier decisions. Expect this; revise old entries explicitly when a new rule lands.
- **Compare across siblings before stubbing.** SDK pages were the clearest example: comparing Node vs Python vs Rust type lists *before* stubbing each surfaced real gaps (missing types) and naming inconsistencies.
- **Verify source code, not just source docs.** Source docs contained genuine inaccuracies (browser SDK telemetry stubs that didn't reflect the code; sdk-browser.mdx omitting real types). Default to grepping the SDK source when types or methods are listed.
- **Hanging pieces parking lot.** Worked well as a low-friction place to stash "this doesn't fit anywhere yet" without blocking forward progress.

### Inline markup conventions used
- `**Consider removing**` — flagged surfaces that may leave the public API.
- `_Confused on this one_` / `_This differs from node, is this okay?_` — italic hand-written reviewer notes embedded in stubs as questions.
- `<Note>` callouts at the top of SDK pages — used to point at the worker that owns a stripped surface (observability, worker-manager).
- `{/* TODO(skills/auto-gen): ... */}` — MDX comment markers for content that needs hand-authored prefix/suffix once auto-gen lands.

These conventions are useful breadcrumbs; preserve them in any future pass.

### Things to assume going in (already true about iii)
- A worker is a worker — no external/built-in distinction.
- All worker-specific content (config, functions, triggers, protocol, error codes) belongs in Worker Docs.
- Logger and telemetry belong to iii-observability.
- Channels belong to iii-worker-manager.
- `iii worker` CLI subcommands and `iii.lock` are iii-level tooling, not Worker Docs.
- Engine config is `config.yaml`. Worker-level config is `iii.worker.yaml`.
- Adapters are deprecated.
- Auto-reconnect is SDK behavior.
- Worker authoring (implementing engine traits, building worker packages) is outside ideal-docs.
- The three primitives are Worker, Function, Trigger — that framing should anchor any new conceptual content.

### Pitfalls observed
- **Naming drift.** `api-reference/` was renamed to `sdk-reference/` mid-pass; `iii-config.yaml` became `config.yaml`. When source docs use older names, normalize to current naming in stubs and note the rename in the decisions log.
- **The "examples" pattern is a trap.** Code-heavy standalone example files almost always either (a) duplicate a tutorial, (b) duplicate a reference page, or (c) belong in Worker Docs. Default to drop unless a unique pattern surfaces.
- **Per-version changelog detail decays fast.** Stubbed at the level of "there is a changelog page" rather than per-version; that scales.
- **"Why this SDK" / "Why this matters" framing has no auto-gen home.** SDK and config-reference pages will be auto-generated; the motivational/positioning prose needs explicit pre/post-append slots in the generator design. Logged in Hanging pieces.

### Adapting this file for the engine/sdks/console pass
The current shape (file list with checkboxes + decisions log + hanging pieces + ground rules) transfers cleanly. Suggestions:
- Replace "every `.mdx` file in `iii-mono/docs/`" with "every public-API surface in `iii-mono/{engine,sdk,console}/`" or whichever scope the next pass picks.
- Likely groupings: per crate/package; per public module; per CLI command; per trigger type; per worker. Group early so decisions can batch when patterns repeat (much like `workers/* — Move to Worker Docs (batch)` here).
- The "Decision log entry" template that emerged: `### <source> — <verdict>\n- <bullets describing where stubs landed, what was dropped, what was flagged>`.
- The status legend (`[ ]` / `[x]` / `[~]`) is the right granularity. Don't add more states.

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

### sdk-reference/
- [x] sdk-reference/disable-telemetry.mdx
- [x] sdk-reference/sandbox.mdx
- [x] sdk-reference/sdk-browser.mdx
- [x] sdk-reference/sdk-node.mdx
- [x] sdk-reference/sdk-python.mdx
- [x] sdk-reference/sdk-rust.mdx

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
- [x] how-to/expose-http-endpoint.mdx
- [x] how-to/manage-state.mdx
- [x] how-to/managing-container-workers.mdx
- [x] how-to/observability-and-logs.mdx
- [x] how-to/react-to-state-changes.mdx
- [x] how-to/reproduce-worker-installs.mdx
- [x] how-to/schedule-cron-task.mdx
- [x] how-to/stream-realtime-data.mdx
- [x] how-to/trigger-actions.mdx
- [x] how-to/trigger-functions-from-cli.mdx
- [x] how-to/use-channels.mdx
- [x] how-to/use-console.mdx
- [x] how-to/use-functions-and-triggers.mdx
- [x] how-to/use-http-middleware.mdx
- [x] how-to/use-iii-in-the-browser.mdx
- [x] how-to/use-named-queues.mdx
- [x] how-to/use-topic-queues.mdx
- [x] how-to/use-trigger-conditions.mdx
- [x] how-to/worker-rbac.mdx

### primitives-and-concepts/
- [x] primitives-and-concepts/discovery.mdx
- [x] primitives-and-concepts/functions-triggers-workers.mdx

### root pages
- [x] index.mdx
- [x] install.mdx
- [x] quickstart.mdx

### tutorials/
- [x] tutorials/index.mdx

### workers/
- [x] workers/index.mdx
- [x] workers/iii-bridge.mdx
- [x] workers/iii-cron.mdx
- [x] workers/iii-exec.mdx
- [x] workers/iii-http.mdx
- [x] workers/iii-observability.mdx
- [x] workers/iii-pubsub.mdx
- [x] workers/iii-queue.mdx
- [x] workers/iii-state.mdx
- [x] workers/iii-stream.mdx
- [x] workers/iii-worker-manager.mdx
- [x] workers/managed-worker-lockfile.mdx

## Decisions log

### advanced/adapters.mdx — Remove
- Adapters are deprecated; remove the page in its current form.
- No new ideal-docs page needed. No "Choosing an Adapter" matrix.
- Per-worker adapter sections (Queue/State/Stream/Cron/PubSub) are worker-specific — flagged for Worker Docs, not migrated here.

### workers/* — Move to Worker Docs (batch)
- All 11 worker pages are per-worker config / functions / triggers / protocol detail; flagged for their respective Worker Docs:
  - `workers/index.mdx` → workers hub
  - `workers/iii-bridge.mdx` → iii-bridge
  - `workers/iii-cron.mdx` → iii-cron
  - `workers/iii-exec.mdx` → iii-exec
  - `workers/iii-http.mdx` → iii-http
  - `workers/iii-observability.mdx` → iii-observability
  - `workers/iii-pubsub.mdx` → iii-pubsub
  - `workers/iii-queue.mdx` → iii-queue
  - `workers/iii-state.mdx` → iii-state
  - `workers/iii-stream.mdx` → iii-stream
  - `workers/iii-worker-manager.mdx` → iii-worker-manager
- **Special case:** `workers/managed-worker-lockfile.mdx` covers `iii.lock` — under the corrected ground rule (iii-level tooling), this is already absorbed by the lockfile / pinning / verify / update stubs on `using-iii/workers.mdx`. No additional ideal-docs work needed; the worker-side equivalent (if any) belongs in iii-worker-manager.

### tutorials/index.mdx — Drop entirely
- Single-paragraph hub; Mintlify Tutorials tab + group structure already serves as the hub.

### quickstart.mdx — Distribute and drop
- Stubs added to `getting-started/quickstart.mdx` covering all 10 steps: Scaffold the project, Start the engine, Start a Python worker, Start a TypeScript worker, Call across languages, Add state, Add HTTP endpoints, How it works, Explore with the console, Agent skills (heading only).
- Drops: code blocks, long inline explanations.

### install.mdx — Distribute and drop
- Stubs added to `getting-started/install.mdx`: Install iii, Verify installation, VS Code extension, Agent skills (heading only — acquisition flow is changing).
- Drops: code blocks, "Next steps" cards.

### index.mdx — Stub on root
- Stub added to root `index.mdx`: "What makes iii different" — the six system-trait highlights.
- Drops: multi-paragraph positioning prose, three-primitives intro (covered in `understanding-iii/index.mdx`), React-analogy callout, Getting Started / Next Steps cards (tabs handle nav).

### primitives-and-concepts/functions-triggers-workers.mdx — Drop entirely
- Already absorbed by `understanding-iii/index.mdx` ("The three primitives", "System overview") and the per-primitive pages (`understanding-iii/functions.mdx`, `triggers.mdx`, `workers.mdx`).

### primitives-and-concepts/discovery.mdx — Distribute and drop
- Stub added to `understanding-iii/engine.mdx`: "Discovery and the live registry".
- `sdk-reference/engine-sdk.mdx` "Engine discovery functions" stub expanded to include `engine::triggers::list`, `engine::trigger-types::list`, and the `engine::functions-available` trigger.
- Drops: "Why this matters" / "What this enables" framing, comparison-with-other-systems passage (config files / DNS / registries / service meshes), code blocks, CardGroup.

### how-to/worker-rbac.mdx — Move to Worker Docs (iii-worker-manager)
- Auth functions, middleware, per-session namespacing, allow/deny lists, trigger-registration RBAC are all iii-worker-manager surface.

### how-to/use-trigger-conditions.mdx — Drop entirely
- Already covered by `understanding-iii/triggers.mdx` "Trigger conditions" + `using-iii/triggers.mdx` "Gate a trigger with a condition".

### how-to/use-topic-queues.mdx — Move to Worker Docs (iii-queue)
- `durable:subscriber` trigger and `iii::durable::publish` are iii-queue surface.

### how-to/use-named-queues.mdx — Move to Worker Docs (iii-queue)
- Named-queue mechanics are iii-queue surface.
- Cross-cutting `TriggerAction.Enqueue` callout already on `using-iii/functions.mdx` and `understanding-iii/functions.mdx`.

### how-to/use-iii-in-the-browser.mdx — Drop, with hanging-piece note
- "Steps" / install / init / examples already covered by `sdk-reference/browser-sdk.mdx` and `how-to/build-a-realtime-todo-app.mdx`.
- "Why use the browser SDK over HTTP" framing has no home in auto-generated SDK reference docs — logged as a hanging piece for the auto-gen tooling design (pre/post-append slots).

### how-to/use-http-middleware.mdx — Move to Worker Docs (iii-http)
- HTTP middleware is iii-http surface.

### how-to/use-functions-and-triggers.mdx — Distribute and drop
- Stub added to `using-iii/functions.mdx`: "Register a function" — explicit symmetry with `using-iii/triggers.mdx` "Register a trigger".
- Function and trigger workflows already covered by existing stubs.

### how-to/use-console.mdx — Drop entirely
- Already absorbed by `using-iii/console.mdx` stubs covering each console UI page.

### how-to/use-channels.mdx — Move to Worker Docs (iii-worker-manager)
- Per channels ground rule.

### how-to/trigger-functions-from-cli.mdx — Already mapped
- Existing stub on `using-iii/cli.mdx` ("Invoking functions through the cli") absorbs this. Expanded to a one-line description naming `iii trigger --function-id/--payload` and `--address`/`--port` for remote engines.

### how-to/trigger-actions.mdx — Distribute and drop
- "Trigger actions" are function-invocation modes despite the name.
- Stubs added to `understanding-iii/functions.mdx`: Invocation modes (sync + void) with a callout noting that workers can provide their own `TriggerAction`s and pointing at iii-queue's `TriggerAction.Enqueue` for queue-routed invocations.
- Stubs added to `using-iii/functions.mdx`: Invoke a function synchronously, Fire-and-forget with `TriggerAction.Void`, plus the same custom-actions / iii-queue callout.
- Drops: code blocks, real-world scenarios, decision flowchart, combining-actions example, common-mistakes section, SDK syntax reference (covered on each SDK page), "Next steps" card.

### how-to/stream-realtime-data.mdx — Move to Worker Docs (iii-stream)
- Stream set/update/delete and change events are iii-stream surface.

### how-to/schedule-cron-task.mdx — Move to Worker Docs (iii-cron)
- Cron trigger type and expression syntax belong with iii-cron.

### how-to/reproduce-worker-installs.mdx — Distribute and drop
- Stubs added to `using-iii/workers.mdx`: Managing workers from the CLI, Pinning workers in `iii.lock`, Verifying the lockfile against config, Updating pinned workers, Pinning binary worker artifacts across platforms.
- Callout added to `using-iii/cli.mdx` pointing to `using-iii/workers` for `iii worker ...` subcommands.
- New ground rule: `iii worker` CLI is iii-level tooling, not Worker Docs.

### how-to/react-to-state-changes.mdx — Move to Worker Docs (iii-state)
- `state:updated` / `state:deleted` triggers are surfaced by iii-state.

### how-to/observability-and-logs.mdx — Move to Worker Docs (iii-observability)
- Per existing ground rule.

### how-to/managing-container-workers.mdx — Distribute and drop (revised)
- Originally flagged for Worker Docs; revised after the `iii worker` CLI ground-rule clarification — these are iii-level tooling.
- Stubs added to `using-iii/workers.mdx`: Managing workers from the CLI (covers add/remove/list/start/stop), Exec into a running worker, Build and publish a worker image.
- Lifecycle / packaging is iii-level; nothing flagged for Worker Docs from this page anymore.

### how-to/manage-state.mdx — Move to Worker Docs (iii-state)
- `state::get` / `state::set` / `state::delete` and scopes belong with iii-state.

### how-to/expose-http-endpoint.mdx — Move to Worker Docs (iii-http)
- iii-http worker config schema + the HTTP trigger type both belong with iii-http.
- Cross-cutting "register a trigger" workflow already stubbed in `using-iii/triggers.mdx`.

### how-to/developing-sandbox-workers.mdx — Move to Worker Docs (iii-sandbox / iii-worker-manager)
- Sandbox is a worker; worker authoring belongs outside ideal-docs.
- No stubs.

### how-to/define-request-response-formats.mdx — Distribute and drop
- Stub added to `using-iii/functions.mdx`: "Define request and response formats".
- "Discover from CLI" piece already covered (`using-iii/cli.mdx`, `sdk-reference/engine-sdk.mdx` discovery functions).
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
- `engine::workers::register` added to the existing engine-discovery-functions stub on `sdk-reference/engine-sdk.mdx`.
- Drops: SDK package table, code snippets, InitOptions table (covered on SDK pages), Reconnection Config field detail (covered by SDK type entries), Python snake_case note, both diagrams, "See also" card.

### architecture/engine.mdx — Distribute and drop
- Stubs added to `understanding-iii/engine.mdx`: Engine responsibilities, Worker disconnect cleanup, Config hot-reload, Architecture-agnostic routing.
- Stub added to `sdk-reference/engine-sdk.mdx`: Engine discovery functions (`engine::functions::list`, `engine::workers::list`, `engine::workers-available`).
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

### sdk-reference/sdk-rust.mdx — Heavy stubs on rust-sdk
- Source verified against `sdk/packages/rust/iii/src/`.
- Logger + Telemetry stripped; observability callout in place.
- Methods trimmed to the cross-SDK pattern: register_worker (with InitOptions), register_function, register_trigger, trigger, shutdown, shutdown_async, register_trigger_type / unregister_trigger_type ("Consider removing"). Dropped: `set_headers`, `register_function_with`, `register_service`, `create_channel`, `get_connection_state`.
- Types: kept Rust-relevant ones (IIIError, IIIConnectionState, HttpAuthConfig, HttpInvocationConfig, HttpMethod, TriggerAction, TriggerRequest, Trigger, FunctionRef, FunctionInfo, TriggerInfo, WorkerInfo, WorkerMetadata, RegisterFunctionMessage, RegisterServiceMessage). Dropped OtelConfig + OTel `ReconnectionConfig` per observability rule.
- Cross-SDK mismatches recorded in Hanging pieces.

### sdk-reference/sdk-python.mdx — Heavy stubs on python-sdk
- Source verified: Python SDK only has one `ReconnectionConfig` (engine WS); no separate OTel reconnection config.
- Logger and Telemetry surfaces stripped from all SDK pages with a callout pointing to iii-observability worker docs (per new ground rule).
- Heavy stubs added on `sdk-reference/python-sdk.mdx`: Installation, Initialization (with `worker` convention), Connection lifecycle, Methods (register_worker w/ InitOptions, register_function, register_trigger, trigger + trigger_async, shutdown + shutdown_async, register_trigger_type / unregister_trigger_type marked "Consider removing"), Types (ReconnectionConfig, HttpInvocationConfig, RegisterFunctionFormat, RegisterServiceInput, RegisterTriggerInput, RegisterTriggerTypeInput, TriggerActionEnqueue, TriggerActionVoid, TriggerRequest, TriggerHandler, IStream).

### sdk-reference/sdk-node.mdx — Heavy stubs on node-sdk
- Source verified: `IIIReconnectionConfig` (engine WS) and `ReconnectionConfig` (OTel telemetry-system) are both real and distinct in node SDK; `OtelConfig`, `HttpAuthConfig`, `HttpInvocationConfig` all exist.
- Heavy stubs added on `sdk-reference/node-sdk.mdx`: Installation, Initialization, Connection lifecycle, all 9 methods, Subpath exports, Logger (debug/info/warn/error), Telemetry suite, full type list.

### sdk-reference/sdk-browser.mdx — Heavy stubs on browser-sdk
- Verified against `sdk/packages/node/iii-browser` source: browser SDK does **not** ship OpenTelemetry. Removed the 5 telemetry stubs incorrectly added during the file 6 pass (Telemetry, Custom spans, Worker metrics, Log emission and subscription, Telemetry utilities).
- Heavy stubs added on `sdk-reference/browser-sdk.mdx`: Installation, Initialization, Connection lifecycle, all 9 methods, Subpath exports, and full type list.
- **Source-doc inaccuracy noted (hanging piece):** iii-mono `sdk-browser.mdx` omits `IIIReconnectionConfig` and `RegisterFunctionFormat` types even though both exist in the browser SDK source.

### sdk-reference/sandbox.mdx — Move to Worker Docs (sandbox)
- Entire page is sandbox-worker-specific (SDK calls, engine config, images, errors, `iii sandbox` CLI subcommands).
- Flagged for Worker Docs. No stubs in ideal-docs.

### sdk-reference/disable-telemetry.mdx — Already mapped
- 1:1 with existing `how-to/disable-telemetry.mdx`. No changes needed.
- When the page is authored, distinguish `III_TELEMETRY_ENABLED` (anonymous usage) from `OTEL_ENABLED` (observability instrumentation).

### advanced/telemetry.mdx — Distribute and drop
- Stubs added to each client SDK page (node, python, rust, browser): Telemetry, Custom spans, Worker metrics, Log emission and subscription, Telemetry utilities.
- Stub added to `sdk-reference/engine-sdk.mdx`: Engine-collected invocation metrics.
- iii-observability worker config + OTel data-flow diagram → flagged for Worker Docs (iii-observability).
- Cross-SDK Comparison matrix dropped.

### advanced/protocol.mdx — Distribute and drop
- Stubs added to `sdk-reference/engine-sdk.mdx`: Message types, Connection flow, Invocation lifecycle, Register function/trigger/invoke/result messages.
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
- "SDK Connection Flow" (lifecycle / heartbeat / reconnect / re-registration) — add stub section in each client SDK reference page (`sdk-reference/node-sdk.mdx`, `python-sdk.mdx`, `rust-sdk.mdx`, `browser-sdk.mdx`).

## Hanging pieces

(Content with no clear home — track here so we don't lose it.)

- **Restore auto-generated config reference (post-outline engineering task).** Pre-Mintlify, `how-to/configure-engine.mdx` was generated from a commented `iii-config.yaml` by a build-time parser + React component (commit `0f925fd2` in iii-mono, files: `docs/content/how-to/iii-config.yaml`, `docs/src/lib/config-parser.ts`, `docs/src/lib/config-toc.ts`, `docs/src/lib/components/ConfigReference.tsx`). Lost in the `feat: mintlify` migration (`79bdd94d`).
  - Goal: regenerate the configuration reference and surface it inside `using-iii/engine.mdx` (or wherever fits best after final structure).
  - Implementation sketch: (1) author a fresh commented `config.yaml` in iii-mono colocated with the engine — current naming (`name: iii-http`, etc.), no deprecated adapter sections; (2) write a generator that emits a Mintlify MDX fragment (pre-build script committing `using-iii/engine.config-reference.generated.mdx`, or a `_snippets/config-reference.mdx` consumed via `<Snippet file="..." />`); (3) transclude into the host page.
  - **Open question — much of this may belong to Worker Docs, not ideal-docs.** With "everything is a worker," what used to read as engine config is largely per-worker config (HTTP host/port/CORS → iii-http; stream auth_function → iii-stream; etc.). Decide during post-outline review whether the reference is one cross-cutting page in ideal-docs or split across each Worker Docs surface.
  - Don't attempt during this migration pass.

- **Source-doc gap:** iii-mono `sdk-reference/sdk-browser.mdx` omits `IIIReconnectionConfig` and `RegisterFunctionFormat` types — both exist in browser SDK source. Stubs included here; flag for the source author.

- **Auto-generated SDK reference docs need pre/post-append slots for narrative content.** SDK reference pages will be auto-generated from code (analogous to the planned config-reference generator). Auto-generation captures the surface — methods, types, signatures — but not motivational/positioning content like "why use the browser SDK over HTTP," migration guidance, when-to-pick-this-SDK, or cross-SDK comparisons.
  - Affected so far: the "why this SDK" framing from `how-to/use-iii-in-the-browser.mdx` has no home in pure auto-gen `sdk-reference/browser-sdk.mdx`.
  - Plan: each SDK reference page should support a hand-written prefix (above auto-gen content) and/or suffix (below) — frontmatter slot, separate `_prefix.mdx`/`_suffix.mdx` partials, or a leading paragraph the generator preserves.
  - Decide during the auto-gen tooling design (same project shape as the config-reference restoration above).

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
