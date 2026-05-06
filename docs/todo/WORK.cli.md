# WORK — CLI

Remaining work on the `iii` CLI surface. Sourced from
[`PROGRESS.md`](../PROGRESS.md) and [`PROGRESS-repo-checks.md`](../PROGRESS-repo-checks.md).

Project rule: [`project-rules/cli.md`](../project-rules/cli.md).
Primary docs page: `using-iii/cli.mdx`. Worker subcommands: `using-iii/workers.mdx`.

## Top-level commands as of audit (engine/src/main.rs:62-122)
- `iii trigger <function-path> [argA="value" argB=5 ...]` — recognized exemption per
  `cli.md`; canonical way to invoke any registered function.
- `iii console [args]` → dispatches to `iii-console` (internal binary).
- `iii cloud [args]` → dispatches to `iii-cloud`.
- `iii worker [args]` → dispatches to `iii-worker`.
- `iii sandbox [args]` → dispatches to `iii-worker sandbox`. **Being removed; replaced
  by `iii trigger sandbox::*`.**
- `iii update [target]` → engine self-update.
- `iii create [args]` → dispatches to `iii-tools create`. **Being removed; replaced by
  `iii project init` and `iii worker init`.**
- Engine flags: `--config`, `--use-default-config`, `--version`.

## 1. `iii cloud` — surface design

**Current state:** stubbed in `using-iii/deployment.mdx` ("iii Cloud deployments") and
cross-linked from `using-iii/cli.mdx` ("Managing iii Cloud deployments"). Both stubs are
single-sentence placeholders.

**Blocker:** see [`CLARIFICATION_NEED.iii-cloud.md`](./CLARIFICATION_NEED.iii-cloud.md).
Subcommands, scope, auth model, and target user are unclear.

- [ ] Define the user-facing surface (subcommands, flags, workflows).
- [ ] Author the `using-iii/deployment.mdx` "iii Cloud deployments" section once the
  surface is known.
- [ ] Add corresponding stubs on `using-iii/cli.mdx` if the surface is large (e.g., a
  per-subcommand list).
- [ ] Decide: is iii Cloud documentation cross-cutting in `deployment.mdx` only, or
  does it warrant a dedicated `using-iii/cloud.mdx` page?

## 2. `iii project init` and `iii worker init` — rename rollout

**Current state:** docs already updated to reference the new commands
(`getting-started/quickstart.mdx` "Scaffold the project" stub names `iii project init`;
`using-iii/workers.mdx` "Scaffold a new worker" stub names `iii worker init`;
`project-rules/cli.md` example list updated). The engine-side dispatcher and `iii-tools`
package still ship `iii create`.

- [ ] **Engine dispatcher** (`engine/src/main.rs`): add `Project { args }` and
  `Worker { args }` (already exists for `worker`) routing for the new commands. See
  [`WORK.engine.md`](./WORK.engine.md).
- [ ] **`iii-tools` package** (`crates/iii-tools/`): add `init` as the canonical command
  (with optional `--template <name>`); deprecate `create`.
- [ ] **`iii-worker` package** (`crates/iii-worker/`): add `init` subcommand under
  `iii worker`. Mirror the `iii project init` shape.
- [ ] Once the new commands ship: remove `iii create` dispatcher entry from the engine
  and the `create` command from `iii-tools`. Update `project-rules/cli.md` to remove the
  intermediate state mention.
- [ ] Verify `crates/scaffolder-core` (shared library) supports both `project` and
  `worker` template types or a `kind` parameter.

## 3. `iii update` — engine self-update

**Current state:** stub on `using-iii/cli.mdx` ("Updating iii itself"); cross-link from
`install.mdx` ("Upgrade iii after install"). Command exists at `engine/src/main.rs:120`,
implementation at `engine/src/cli/update.rs`.

- [ ] Author content on `using-iii/cli.mdx`: what `[target]` accepts (engine? specific
  managed binaries?); upgrade safety; rollback story.
- [ ] Confirm with engineering whether `iii update` updates managed worker binaries or
  only the engine itself; the help text says "iii and managed binaries."

## 4. `iii sandbox` — removal

**Current state:** subcommands (`run`, `create`, `exec`, `list`, `stop`, `upload`,
`download`) being removed and replaced by `iii trigger sandbox::*`. All sandbox docs
route to Worker Docs (sandbox is a worker — see
[`CLARIFICATION_NEED.sandbox-worker.md`](./CLARIFICATION_NEED.sandbox-worker.md)).

- [ ] **Code change** (engine + iii-worker): expose sandbox functions
  (`sandbox::run`, `sandbox::create`, `sandbox::exec`, `sandbox::list`, `sandbox::stop`,
  `sandbox::upload`, `sandbox::download`) via the responsible worker so they're
  invokable through `iii trigger sandbox::*`.
- [ ] **Code change** (engine): remove the `Sandbox { args }` top-level command from
  `engine/src/main.rs:115` once functions are wired up.
- [ ] No ideal-docs work; sandbox content is Worker Docs.

## 5. `iii trigger` syntax — propagate

**Current state:** recognized exemption documented in `project-rules/cli.md`. The
`using-iii/cli.mdx` "Invoking functions through the cli" stub still uses the old
`--function-id`/`--payload` flag form.

- [ ] When authoring `using-iii/cli.mdx`, update the "Invoking functions through the
  cli" section to use the canonical `iii trigger <function-path> [argA="value" argB=5
  argC="other value"] [--address …] [--port …]` form. Keep the existing remote-engine
  flags (`--address`, `--port`).
- [ ] When authoring the "Publishing triggers through the cli" section, decide if the
  new syntax absorbs it (one verb does both) or whether they remain distinct.

## 6. CLI naming review (engineering decisions)

These flow through CLI surfaces but are engineering naming decisions, not docs work:

- [ ] `iii-http` vs `iii-http-functions` worker naming (could rename to `iii-http-client`
  / `iii-http-outbound` for clarity).
- [ ] `engine::` prefix on observability worker functions (could rename to `obs::*` /
  `iii-observability::*`).
- [ ] `iii-engine-functions` worker name (could rename to better reflect "engine
  discovery surface" if that's its primary user-facing role).

See [`WORK.engine.md`](./WORK.engine.md) for the same items as engine-side cleanup.

## 7. `iii worker` subcommands — content authoring

**Current state:** `using-iii/workers.mdx` has stubs for every `iii worker` subcommand
(per `project-rules/cli.md` rule that subcommands live with their noun page). Each is a
one-line stub.

`iii worker` subcommands that need authoring:
- `iii worker init` (new — Scaffold a new worker)
- `iii worker add`
- `iii worker remove`
- `iii worker reinstall`
- `iii worker update [name]`
- `iii worker clear [name]`
- `iii worker start`
- `iii worker stop`
- `iii worker restart`
- `iii worker list`
- `iii worker sync` (`--frozen`)
- `iii worker verify` (`--strict`)
- `iii worker status [name]` (`--no-watch`)
- `iii worker logs [name]` (`--follow`, `--address`, `--port`)
- `iii worker exec <name> -- <cmd>` (`--env`, `--workdir`, `--tty`, `--no-tty`,
  `--timeout`)

- [ ] Author each section on `using-iii/workers.mdx` once the surface stabilizes (the
  init rename should land before authoring to avoid rewrites).

## Notes / dependencies

- §1 (iii cloud) is blocked on the surface clarification.
- §2 (init rename) is blocked on engine + iii-tools + iii-worker code changes.
- §3 (iii update) is authoring-ready; depends only on engineering confirmation about
  scope of `[target]`.
- §4 (iii sandbox removal) depends on the sandbox worker exposing functions for `iii
  trigger sandbox::*`.
- §5 (iii trigger syntax propagation) is a docs authoring task.
- §6 (naming reviews) are engineering decisions; docs side already reflects code reality.
- §7 (iii worker subcommand authoring) is sequenced after the init rename lands.
