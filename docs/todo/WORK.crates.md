# WORK — Crates

Crates monorepo work. Sourced from [`PROGRESS.md`](../PROGRESS.md) and
[`PROGRESS-repo-checks.md`](../PROGRESS-repo-checks.md).

Repo: `iii-temp/crates/`.

## Crates surveyed (10)

| Crate              | Type                                | Routing                                               |
| ------------------ | ----------------------------------- | ----------------------------------------------------- |
| `iii-tools`        | Binary (`iii`)                      | ideal-docs (scaffolder; renaming)                     |
| `iii-worker`       | Binary (`iii worker`/`iii sandbox`) | ideal-docs                                            |
| `iii-supervisor`   | Library                             | Internal infra                                        |
| `iii-init`         | Library + guest binary              | Internal infra (PID 1 in microVM)                     |
| `iii-network`      | Library                             | Internal infra (smoltcp networking for VM sandboxes)  |
| `iii-filesystem`   | Library                             | Internal infra (passthrough FS for VM sandboxes)      |
| `iii-shell-proto`  | Library                             | Internal infra (`iii worker exec` wire protocol)      |
| `iii-shell-client` | Library                             | Internal infra (`iii worker exec` host client)        |
| `scaffolder-core`  | Library                             | Internal infra (shared by iii-tools / motia-tools)    |
| `motia-tools`      | Binary (`motia`)                    | Out of scope (declared in `project-rules/general.md`) |

## 1. `iii-tools` — rename for `iii project init`

**Source:** PROGRESS-repo-checks.md `iii create` rename hanging piece.

`iii create` is being removed and replaced by:

- `iii project init [--template <name>]`
- `iii worker init [--template <name>]`

For `iii-tools` specifically:

- [ ] Add `init` as the canonical command (with `--template <name>` and other existing flags). The
      legacy `create` command should be deprecated then removed.
- [ ] Confirm the binary name stays `iii` (since `iii-tools` registers `iii` as the binary, and the
      engine dispatches `iii project init` to whichever binary handles it).
- [ ] If `iii-tools` is no longer the right home (since the engine dispatcher routes
      `iii project init` directly), evaluate splitting / renaming the crate. Coordinate with
      [`WORK.engine.md`](./WORK.engine.md) §1.

## 2. `iii-worker` — add `iii worker init`

**Source:** same rename hanging piece.

- [ ] Add an `init` subcommand to `iii-worker` that accepts `--template <name>` (and other relevant
      flags). Mirror the shape of `iii project init`.
- [ ] Coordinate with [`WORK.engine.md`](./WORK.engine.md) §1 for the dispatcher route.

## 3. `scaffolder-core` — support both project and worker templates

**Source:** PROGRESS-repo-checks.md crates survey.

Currently shared by `iii-tools` (project scaffolding) and `motia-tools` (Motia scaffolding). For the
rename, it likely needs to support a worker-template kind.

- [ ] Confirm `scaffolder-core` supports both `project` and `worker` template kinds (or add a `kind`
      parameter to its public API).
- [ ] If the abstraction needs to grow, factor accordingly. Both `iii-tools` and `iii-worker` will
      consume it.

## 4. `motia-tools` — legacy/active decision

**Source:** PROGRESS-repo-checks.md top finding #6 + hanging piece.

`crates/motia-tools` exposes `motia create` using the same `scaffolder-core` as `iii-tools` but
points at `motia.dev/docs`. Out of scope for ideal-docs (declared in `project-rules/general.md`).

- [ ] Decide: legacy-keep, deprecate, or remove from monorepo. Either way, no ideal-docs work — but
      the decision affects whether `scaffolder-core` retains its product-agnostic abstraction.
- [ ] If kept: leave as-is, no further action.
- [ ] If deprecated: add deprecation notice in `motia-tools` README; plan removal timeline.
- [ ] If removed: remove `crates/motia-tools/`; update `scaffolder-core` to drop the abstraction
      layer for product-agnostic use.

## 5. Internal-infra crates — no docs work

These crates are internal infrastructure. Confirmed during the audit that none need ideal-docs
targets:

- `iii-supervisor` — In-VM process supervisor library.
- `iii-init` — PID 1 init binary for iii microVM workers (guest-side; not user-facing).
- `iii-network` — Userspace TCP/IP networking for iii worker VM sandboxes.
- `iii-filesystem` — Filesystem backends for iii worker VM sandboxes.
- `iii-shell-proto` — Wire protocol for the iii shell-exec channel.
- `iii-shell-client` — Async pipe-mode client for the iii shell-exec channel.
- `scaffolder-core` — Project scaffolding shared library.

These surface indirectly via `iii sandbox` (now → Worker Docs per top finding #3 resolution; see
[`WORK.workers.md`](./WORK.workers.md)) and `iii worker exec` (already stubbed in
`using-iii/workers.mdx`).

If any of these become user-facing in the future, add to a WORK file at that point.

## 6. Dead code detection

See [`WORK.dead-code.md`](./WORK.dead-code.md) for the cross-cutting initiative. Crates-specific
items:

- [ ] Enable `#![warn(dead_code, unused_imports, unused_variables, unused_must_use)]` at every crate
      root in `crates/*`.
- [ ] Add `cargo machete` to the workspace CI; backstop with weekly `cargo +nightly udeps` over the
      whole workspace.
- [ ] After §1 / §2 (`init` rename), confirm the legacy `create` command code path is fully removed
      from `iii-tools` and not re-exported from `scaffolder-core`.
- [ ] If §4 resolves to remove `motia-tools`, confirm `scaffolder-core` drops the product-agnostic
      abstraction and any now-dead trait impls.

## Notes / dependencies

- `iii-tools` and `iii-worker` rename work (§1, §2) is coordinated with
  [`WORK.engine.md`](./WORK.engine.md) §1 (dispatcher changes).
- `scaffolder-core` (§3) may need API changes; confirm before §1/§2 ship.
- `motia-tools` decision (§4) is independent.
- Internal-infra crates (§5) have no work pending.
