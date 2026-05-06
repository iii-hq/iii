# WORK — ideal-docs (the iii docs site)

Authoring work for the iii docs (this site). Sourced from
[`PROGRESS.md`](../PROGRESS.md) and [`PROGRESS-repo-checks.md`](../PROGRESS-repo-checks.md).

The migration phase produced section stubs (`## heading` + ≤1-sentence description)
across all pages. This file tracks the **authoring phase** — turning stubs into prose —
and any structural follow-ups.

Project rules in [`project-rules/`](../project-rules/) apply throughout.

## 1. Authoring scope: every current stub

All 37 current `.mdx` pages are placeholders awaiting authoring. Each page's
`description: "Placeholder."` frontmatter line should also be updated when authored.

### Root pages
- [ ] `index.mdx` — Welcome (lead language ported from website; "What makes iii
  different" stub remains).
- [ ] `install.mdx` — Install iii, Verify installation, Upgrade after install, VS Code
  extension, Agent skills.
- [ ] `quickstart.mdx` — 10-section Scaffold → Engine → Worker → Cross-language → State
  → HTTP → How it works → Console → Skills.
- [ ] `changelog/index.mdx` — single-line page, very minimal.

### Using iii
- [ ] `using-iii/workers.mdx` — managing workers (CLI subcommands), iii.lock pinning,
  scaffold-a-worker, exec, build/publish, "Did you know?" introspection callout.
- [ ] `using-iii/engine.mdx` — engine config (placeholder for auto-gen reference).
- [ ] `using-iii/functions.mdx` — register, invoke, fire-and-forget, request/response
  formats.
- [ ] `using-iii/triggers.mdx` — register, bind multiple, gate with condition,
  unregister.
- [ ] `using-iii/console.mdx` — per-page (Workers, Functions, Triggers, States, Streams,
  Queues, Traces, Logs, Flow, Configuration); introspection callout already in place.
- [ ] `using-iii/cli.mdx` — engine flags, iii cloud (depends on
  [`WORK.cli.md`](./WORK.cli.md) §1), iii update, iii trigger syntax (per
  `project-rules/cli.md`).
- [ ] `using-iii/deployment.mdx` — Docker, reverse proxy, hardening, multi-instance,
  health checks, ephemeral workers, iii Cloud (depends on [`WORK.cli.md`](./WORK.cli.md)
  §1).

### Understanding iii
- [ ] `understanding-iii/index.mdx` — three primitives, system overview.
- [ ] `understanding-iii/workers.mdx` — concept, lifecycle states, isolation.
- [ ] `understanding-iii/functions.mdx` — what a function is, identifiers, invocation
  modes (sync, void, custom TriggerActions).
- [ ] `understanding-iii/triggers.mdx` — components, pipeline, lifecycle, multiple
  triggers per function, conditions.
- [ ] `understanding-iii/engine.mdx` — startup flow, responsibilities, worker
  disconnect cleanup, hot-reload, architecture-agnostic routing, discovery callout.

### Expanding iii
- [ ] `expanding-iii/workers.mdx` — how workers expand iii, configuring workers, what a
  worker contributes.
- [ ] `expanding-iii/registry.mdx` — TBD (no migrated content; needs scope).

### How-tos
- [ ] `how-to/build-a-realtime-todo-app.mdx` — engine config, backend (worker, auth,
  stream, functions), frontend (connection, real-time hook), key concepts.

### Patterns
- [ ] `patterns/adapter-pattern.mdx` — **adapters are deprecated** (per
  `project-rules/general.md`); decide whether this page should be dropped, repurposed,
  or rewritten as a different pattern.
- [ ] `patterns/reactive-state-pattern.mdx` — TBD scope.

### SDK reference
- [ ] `sdk-reference/node-sdk.mdx` — Installation, Initialization, Connection lifecycle,
  9 methods, Subpath exports, full type list.
- [ ] `sdk-reference/python-sdk.mdx` — same shape, with async variants.
- [ ] `sdk-reference/rust-sdk.mdx` — same shape, with idiomatic differences (natively
  async trigger; `shutdown_async`).
- [ ] `sdk-reference/browser-sdk.mdx` — same shape, no OTel; auto-gen prefix slot for
  "why the browser SDK over plain HTTP."
- [ ] `sdk-reference/engine-sdk.mdx` — message types, connection flow, invocation
  lifecycle, register/invoke/result message shapes, engine-collected metrics, engine
  discovery functions, disable telemetry, observability callout.

### Tutorials (Diataxis tutorials tab)
- [ ] `tutorials/reactive-crud/{overview,model-and-store,crud-endpoints,realtime-subscriptions}.mdx`.
- [ ] `tutorials/incremental-adoption/{overview,wrap-existing-api,offload-to-queue,migrate-persistence}.mdx`.
- [ ] `tutorials/build-an-agent/{overview,define-tools,orchestration-loop,memory-and-state}.mdx`.

## 2. Voice and style

When authoring, apply:
- [`project-rules/voice.md`](../project-rules/voice.md) — declarative, confident,
  paradigm-shift framing; avoid marketing fluff, tutorial-speak, and hedging.
- [`project-rules/docs.md`](../project-rules/docs.md) — `expanding-iii/` is about
  expanding an iii system with workers, not authoring workers.
- [`project-rules/general.md`](../project-rules/general.md) — adapters are deprecated;
  no "step" / "steps" terminology in worker/function context.
- Vale rules in `styles/Terminology/` — slop terms enforced as `level: error`.

## 3. Cross-cutting structural items

### Agent skills section
- All five entry pages (`index.mdx`, `install.mdx`, `quickstart.mdx`,
  `getting-started/quickstart.mdx`-now-`quickstart.mdx`) reference "Agent skills" but
  acquisition flow is changing.
- [ ] Confirm the skill bundle install path (`skillkit` vs `npx skillkit`) and update
  the stub once stable. Coordinate with [`WORK.skills.md`](./WORK.skills.md).

### Worker Docs callout convention
- Per the new general rule in `project-rules/workers.md`: signpost broadly-important
  Worker Docs surfaces via callouts in ideal-docs.
- Already placed for introspection (`understanding-iii/engine.mdx`,
  `sdk-reference/engine-sdk.mdx`, `using-iii/console.mdx`, `using-iii/workers.mdx`).
- Already placed for logger/telemetry → iii-observability (SDK pages).
- Already placed for channels → iii-worker-manager (SDK pages).
- [ ] When authoring, watch for new surfaces that meet the bar; add callouts as needed.

### Cross-page links
- [ ] When authoring, replace any `(#)` placeholder hrefs in callouts with real Worker
  Docs URLs once those URLs exist. Search for `\(#\)` markdown links across the docs.

## 4. Auto-generated content slots

Per [`WORK.auto-gen.md`](./WORK.auto-gen.md):

### Config reference
- [ ] `using-iii/engine.mdx` "Engine configuration" stub is a placeholder for the
  eventual generated content (per `project-rules/config.md`). Don't hand-author per-field
  schema content.

### SDK reference
- [ ] Each `sdk-reference/<lang>-sdk.mdx` page should support a hand-written prefix and
  optional suffix once auto-gen lands. Currently a `{/* TODO(skills/auto-gen) */}`
  marker only on `browser-sdk.mdx`.

## 5. Inline markup conventions to preserve

Per PROGRESS.md "Inline markup conventions used":

- `**Consider removing**` — surfaces that may leave the public API.
- `_Confused on this one_` / `_This differs from node, is this okay?_` — italic
  reviewer notes embedded as questions.
- `<Note>` callouts at the top of SDK pages pointing at the worker that owns a stripped
  surface.
- `{/* TODO(skills/auto-gen): ... */}` — MDX comment markers for content that needs
  hand-authored prefix/suffix once auto-gen lands.

When authoring, preserve these as they are; resolve them only after the underlying
question is resolved.

## 6. Hanging items from PROGRESS.md

### Auto-generated config reference (post-outline engineering task)
- [ ] Restore the config reference. See [`WORK.auto-gen.md`](./WORK.auto-gen.md) for the
  full plan.

### Auto-generated SDK reference docs need pre/post-append slots
- [ ] Design and implement. See [`WORK.auto-gen.md`](./WORK.auto-gen.md).

## 7. Mintlify deployment

- Site is deployed to `iii-docs.vercel.app`, proxied from website `/docs/*`.
- Doc paths now match website (`/install`, `/quickstart` at root) — no redirects needed.
- [ ] Confirm Mintlify build passes after each authoring round.
- [ ] Confirm vale lint passes (slop terminology rules; per `.vale.ini`).

## Notes / dependencies

- Authoring depends on Worker Docs landing for cross-page callouts and links to be
  meaningful — see [`WORK.workers.md`](./WORK.workers.md).
- Auto-gen content (config reference, SDK reference) depends on tooling — see
  [`WORK.auto-gen.md`](./WORK.auto-gen.md).
- iii cloud / iii update / iii project init / iii worker init authoring depends on the
  engine code work landing — see [`WORK.cli.md`](./WORK.cli.md) and
  [`WORK.engine.md`](./WORK.engine.md).
- Patterns pages (`patterns/adapter-pattern.mdx`, `reactive-state-pattern.mdx`) need
  scope decisions before authoring — adapter-pattern is at risk of removal given the
  adapter deprecation rule.
