# WORK — Engine

Engine-side code work. Sourced from [`PROGRESS.md`](../PROGRESS.md) and
[`PROGRESS-repo-checks.md`](../PROGRESS-repo-checks.md).

This file captures engine code changes the audit surfaced — naming reviews, CLI dispatcher changes,
README/config normalizations, and architectural decisions. None of these are docs-authoring work;
they're hand-offs to the engine team.

Repo: `iii-temp/engine/`.

## 1. CLI dispatcher changes

### `iii project init` and `iii worker init` — new commands

**Source:** PROGRESS-repo-checks.md `iii create` rename hanging piece.

`iii create` (currently shipped via `crates/iii-tools`) is being removed. Replacement:

- `iii project init [--template <name>]` for project scaffolding.
- `iii worker init [--template <name>]` for worker scaffolding.

Both default to a base template when `--template` is omitted.

- [ ] Add a `Project { args }` top-level command to `engine/src/main.rs` that dispatches to the new
      `iii project init` (likely a new binary or a refactored `iii-tools`).
- [ ] Add an `init` subcommand to the `iii worker` dispatcher (already routes to `iii-worker`).
- [ ] Update `crates/iii-tools` to expose `iii project init`.
- [ ] Update `crates/iii-worker` to expose `iii worker init`.
- [ ] Confirm `crates/scaffolder-core` supports both project and worker template kinds (or a `kind`
      parameter).
- [ ] Once the new commands ship and stabilize: remove `iii create` from `engine/src/main.rs`
      dispatcher and the `create` command from `iii-tools`.

### `iii sandbox` — removal

**Source:** PROGRESS-repo-checks.md top finding #3 resolution.

Current subcommands (`run`, `create`, `exec`, `list`, `stop`, `upload`, `download`) are being
replaced by `iii trigger sandbox::*` invocations.

- [ ] Expose sandbox functions through the responsible worker so they can be invoked via
      `iii trigger sandbox::run`, etc. See
      [`CLARIFICATION_NEED.sandbox-worker.md`](./CLARIFICATION_NEED.sandbox-worker.md) for
      worker-name question.
- [ ] Remove the `Sandbox { args }` top-level command from `engine/src/main.rs:115` once the
      function-based path is live.

### `iii cloud` — surface implementation

**Source:** PROGRESS-repo-checks.md top finding #1.

Currently dispatches to an `iii-cloud` binary at `engine/src/main.rs:87`. The user-facing surface
(subcommands, workflows) is undefined.

- [ ] Implement the `iii-cloud` binary surface (subcommands, flags, auth model). See
      [`CLARIFICATION_NEED.iii-cloud.md`](./CLARIFICATION_NEED.iii-cloud.md).
- [ ] Coordinate with docs once the surface is defined (`using-iii/deployment.mdx` iii Cloud section
      is currently a single-sentence placeholder).

### `iii update` — confirm scope of `[target]`

**Source:** PROGRESS-repo-checks.md top finding #2.

Command exists at `engine/src/main.rs:120`; help text says "Update iii and managed binaries."
`[target]` accepts… what?

- [ ] Document the accepted values for `[target]` (engine? specific managed binaries? worker
      images?).
- [ ] Confirm the upgrade safety story (rollback? in-place? signature check?).

## 2. Adapter cleanup

**Source:** PROGRESS-repo-checks.md top finding #8 + hanging-pieces punch list.

`project-rules/general.md` confirms adapters are genuinely deprecated. Engine still ships:

### Adapter directories to remove

- [ ] `engine/src/workers/stream/adapters/`
- [ ] `engine/src/workers/state/adapters/`
- [ ] `engine/src/workers/queue/adapters/`
- [ ] `engine/src/workers/pubsub/adapters/`
- [ ] `engine/src/workers/cron/adapters/`

### Adapter blocks in example configs to remove

- [ ] `engine/config.yaml` lines 6-13 (iii-stream adapter), 17-21 (iii-state adapter), 150-153
      (iii-queue adapter), 157-158 (iii-pubsub adapter), 160-163 (iii-cron adapter).
- [ ] `engine/config-remote-kv.yaml` lines 7-9.
- [ ] `engine/config.prod.yaml` (any adapter blocks).

After cleanup, ideal-docs continues to drop adapter content; Worker Docs does not document adapter
blocks.

## 3. `iii-config.yaml` → `config.yaml` normalization

**Source:** PROGRESS-repo-checks.md top finding #9 + hanging-pieces punch list.

`project-rules/config.md` says engine config is `config.yaml`. Stale `iii-config.yaml` references
in:

- [ ] `engine/README.md:48,49,75,85,133,140` (first-touch instructions; highest priority).
- [ ] `engine/src/workers/queue/README.md:59`.
- [ ] `engine/src/workers/rest_api/README.md:93`.
- [ ] `engine/src/workers/worker/README.md:7`.

Per-worker READMEs may move to Worker Docs; if they do, normalize during that move instead of
in-place.

## 4. Naming reviews (engineering decisions)

**Source:** PROGRESS-repo-checks.md hanging pieces — multiple engineering-review items. Docs side
already reflects code reality; these are engineering naming decisions that may or may not happen.

### `iii-engine-functions` framing

- ideal-docs surfaces this worker as "Engine SDK" on `sdk-reference/engine-sdk.mdx` (a friendly
  user-facing alias).
- [ ] Decide: keep the alias (docs) and the worker name (code), or rename for symmetry?
- Specifically: does `engine::*` prefix stay even after the worker is renamed? Are discovery
  functions ever expected to move out of iii-engine-functions?

### `engine::` prefix on observability-worker functions

- `engine::log::*`, `engine::traces::*`, `engine::metrics::*`, `engine::logs::*`,
  `engine::baggage::*`, `engine::sampling::rules`, `engine::health::check`, `engine::alerts::*`,
  `engine::rollups::list` are all observability-worker functions but engine-prefixed (suggesting
  "the engine itself" rather than a worker).
- [ ] Decide whether the prefix should change (e.g., `obs::*`, `iii-observability::*`) or stay
      as-is.
- Docs side already routes these to iii-observability Worker Docs per `workers.md`.

### `engine::channels::create` registration location

- Channels-as-concept belong to iii-worker-manager (`workers.md` rule), but the channel-creation
  function is registered in iii-engine-functions (`workers/engine_fn/mod.rs:494-615`).
- [ ] Decide: move registration to iii-worker-manager, or keep current split (iii-engine-functions
      is the discovery-function home regardless of conceptual owner)?

### `iii-http` vs `iii-http-functions`

- Two real workers with confusingly similar names. `iii-http` is the inbound REST API worker;
  `iii-http-functions` is the outbound HTTP-invocation surface.
- [ ] Decide: rename `iii-http-functions` (e.g., `iii-http-client`, `iii-http-outbound`) for
      unambiguous user-facing reference, or keep as-is?
- Docs side already disambiguates in `project-rules/workers.md`.

### Introspection consolidation

- Currently introspection is split across iii-engine-functions (lists / discovery / channels) and
  iii-observability (traces / logs / metrics / alerts).
- User flagged: "this may need to improve" and "be moved to a worker instead of from iii itself."
- [ ] Decide: consolidate the surfaces into a dedicated introspection worker, or keep the current
      split and improve discovery within Worker Docs?

## 5. Protocol-level items

### `StreamChannelRef` and `ChannelDirection` placement

- Defined in `engine/src/protocol.rs:200-217`.
- Per `workers.md`, channels belong to iii-worker-manager Worker Docs; the protocol-level
  definitions live in the engine.
- [ ] Decide: are these protocol-level types stable enough to expose, or should they be internal and
      re-exported only via iii-worker-manager? Affects how Worker Docs references the engine
      protocol page.

## 6. Auto-generation pre-conditions

**Source:** PROGRESS.md hanging pieces.

The auto-gen config reference (a planned re-implementation of the pre-Mintlify
`docs/content/how-to/iii-config.yaml` parser; commit `0f925fd2` in iii-mono had the prior
implementation) needs:

- [ ] A fresh commented `config.yaml` colocated with the engine that uses current naming
      (`name: iii-http`, etc.) and has no deprecated adapter sections.
- [ ] A generator that emits a Mintlify MDX fragment for the config reference.

See [`WORK.auto-gen.md`](./WORK.auto-gen.md) for the full design.

## 7. Dead code detection

See [`WORK.dead-code.md`](./WORK.dead-code.md) for the cross-cutting initiative. Engine-specific
items:

- [ ] Enable `#![warn(dead_code, unused_imports, unused_variables, unused_must_use)]` at the engine
      crate root and gate release CI with `RUSTFLAGS="-D warnings"`.
- [ ] Add `cargo machete` to engine CI; backstop with weekly `cargo +nightly udeps`.
- [ ] After §2 (adapter cleanup) lands, confirm no orphaned `mod.rs` / config references remain.
- [ ] After §1 (`iii create` / `iii sandbox` removal) lands, confirm no dead dispatcher arms or
      unused helper modules linger in `engine/src/main.rs` and `engine/src/cli/`.

## Notes / dependencies

- Adapter cleanup (§2) and `iii-config.yaml` normalization (§3) are independent and can ship
  anytime.
- CLI dispatcher changes (§1) for `iii project init` / `iii worker init` are coordinated with
  [`WORK.cli.md`](./WORK.cli.md) and [`WORK.crates.md`](./WORK.crates.md).
- Naming reviews (§4) may cascade into Worker Docs filenames and the iii-temp source rename —
  coordinate before Worker Docs authoring begins.
- Auto-gen pre-conditions (§6) are blocked on the auto-gen tooling design landing.
