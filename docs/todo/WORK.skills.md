# WORK — Agent skills

Remaining work on iii agent skill bundles. Sourced from
[`PROGRESS.md`](../PROGRESS.md) and [`PROGRESS-repo-checks.md`](../PROGRESS-repo-checks.md).

Skills are user-invokable from coding agents (`skillkit read <skill-name>` or
`npx skillkit read <skill-name>`). They live outside ideal-docs; this file tracks the
content alignment work each skill needs once Worker Docs and SDK changes settle.

Skill bundles known to exist (per `CLAUDE.md` skills_system block):
- `iii-getting-started`
- `iii-browser-sdk`
- `iii-http-middleware`

## 1. `iii-getting-started`

**Purpose (per CLAUDE.md):** Install the iii engine, set up your first worker, get a
working backend running.

### Alignment work
- [ ] Update install instructions to match `install.mdx` and `quickstart.mdx` (paths
  recently switched to root). Skills should reference `iii.dev/docs/install` and
  `iii.dev/docs/quickstart`.
- [ ] Replace any references to `iii create` with `iii project init` (and
  `iii worker init` for new workers). Pending the engine-side rename — see
  [`WORK.cli.md`](./WORK.cli.md) and [`WORK.engine.md`](./WORK.engine.md).
- [ ] Add introspection guidance: "after the engine is up, you can ask it what it knows
  via `engine::workers::list`, `engine::functions::list`, `engine::triggers::list`."
  Cross-link to iii-engine-functions Worker Docs once it lands.
- [ ] Add observability guidance: "to see traces and logs, query `engine::traces::list`,
  `engine::logs::list`, etc., or open `iii console`." Cross-link to iii-observability
  Worker Docs once it lands.
- [ ] Reflect the `iii noun verb` CLI pattern and the `iii trigger <function-path>
  [argA="value" ...]` syntax (per `project-rules/cli.md`).

## 2. `iii-browser-sdk`

**Purpose (per CLAUDE.md):** Browser SDK for connecting to the iii engine from web
applications via WebSocket — registering functions, invoking triggers, consuming
streams from the frontend.

### Alignment work
- [ ] Match the current public Browser SDK surface (`sdk/packages/node/iii-browser/src/index.ts`).
  Skills should NOT reference types not in the package's exports.
  - [ ] Currently NOT exported: `IIIReconnectionConfig`, `RegisterFunctionFormat`. If the
    SDK adds re-exports (see [`WORK.sdk.md`](./WORK.sdk.md) §2), update skill content.
- [ ] Strip channel-related and telemetry-related guidance per `sdks.md` — those belong
  in iii-worker-manager and iii-observability Worker Docs respectively. Browser SDK
  doesn't ship OTel today.
- [ ] Cross-link to iii-stream Worker Docs for stream consumption patterns once that
  Worker Docs lands.
- [ ] Include the "why use the browser SDK over plain HTTP" framing — currently no home
  in the auto-gen SDK reference; a skill bundle is a natural place for this narrative.

## 3. `iii-http-middleware`

**Purpose (per CLAUDE.md):** Registers engine-level middleware functions that run before
HTTP handlers (auth, logging, rate limiting, pre-handler logic).

### Alignment work
- [ ] Whole skill is iii-http surface. Content depends on iii-http Worker Docs landing
  first — see [`WORK.workers.md`](./WORK.workers.md).
- [ ] Once iii-http Worker Docs lands, refactor this skill to reference it as the
  canonical source rather than duplicating content. Skill should focus on the agent-side
  integration pattern, not re-explain the worker.
- [ ] Reconfirm the term "engine-level middleware" is right — the iii-http worker owns
  middleware registration; "engine-level" predates the everything-is-a-worker rename.

## 4. New skill bundles to consider

The repo audit surfaced two surfaces that are critical for agents and don't yet have a
skill bundle:

### `iii-introspection` (proposed)
- "Critical for skills for agents" per user feedback.
- Covers: how to ask iii what it knows (workers, functions, triggers, trigger types);
  how to query traces / logs / metrics; how to subscribe to live events.
- Sources: iii-engine-functions Worker Docs (lists/discovery) + iii-observability
  Worker Docs (traces/logs/metrics).
- Includes the streaming-traces-as-triggers reactive pattern flagged in
  PROGRESS-repo-checks.md.
- [ ] Decide whether this is a separate skill or folded into `iii-getting-started`.

### `iii-sandbox` (proposed)
- The `iii sandbox` subcommand is being removed and replaced by `iii trigger sandbox::*`
  (see [`WORK.cli.md`](./WORK.cli.md)). Sandbox docs route to Worker Docs.
- Agent use case: spawning ephemeral sandbox VMs for AI-generated code.
- [ ] Decide whether sandbox use deserves its own skill bundle or is covered by
  `iii-getting-started` + the Worker Docs reference.

## 5. Cross-cutting

- [ ] All skills should reference iii-engine-functions and iii-observability Worker Docs
  once those docs land (the introspection callout pattern from
  `project-rules/workers.md` applies inside skill content too).
- [ ] All skills should follow `project-rules/voice.md` — declarative, confident,
  paradigm-shift framing; no marketing fluff or tutorial-speak.
- [ ] All skills should follow `project-rules/cli.md` — `iii noun verb` syntax,
  `iii trigger <function-path> [argA=...]` exemption.

## Notes / dependencies

- iii-getting-started is blocked on the `iii project init` / `iii worker init` rename
  shipping (see [`WORK.engine.md`](./WORK.engine.md)).
- iii-browser-sdk is blocked on browser SDK exports decision (see
  [`WORK.sdk.md`](./WORK.sdk.md) §2).
- iii-http-middleware is blocked on iii-http Worker Docs landing (see
  [`WORK.workers.md`](./WORK.workers.md)).
- New skill bundles (introspection, sandbox) are blocked on the relevant Worker Docs
  landing.
- A general skill-authoring guide may be needed; not in scope for ideal-docs.
