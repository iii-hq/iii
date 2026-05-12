# WORK — Dead code detection

Cross-cutting tracker for setting up automated dead-code detection across the iii monorepos and
keeping deprecated-but-still-shipped surfaces from accumulating.

Project rule: [`project-rules/general.md`](../project-rules/general.md) ("Avoid dead code").

Per-surface pointers:

- Engine: [`WORK.engine.md`](./WORK.engine.md) §7.
- SDKs: [`WORK.sdk.md`](./WORK.sdk.md) §9.
- CLI: [`WORK.cli.md`](./WORK.cli.md) §8.
- Console: [`WORK.console.md`](./WORK.console.md) "Dead code".
- Crates: [`WORK.crates.md`](./WORK.crates.md) §6.

## Scope

What counts as "dead code" for this initiative:

1. Unused public exports / unreferenced symbols (functions, types, modules).
2. Unused dependencies in `Cargo.toml`, `package.json`, `pyproject.toml`.
3. Orphaned files, modules, or whole directories no longer wired into the build.
4. Deprecated-but-still-shipped surfaces tracked elsewhere (adapters, `iii create`, `iii sandbox`,
   forbidden SDK exports, `register_function_with`, etc.) — indexed in §3 below so the removal
   cadence is visible in one place.

## 1. Tooling per language

Pick one tool per category per language; standardize across all repos.

### Rust (`engine/`, `crates/*`, `console/console-rust/`)

- [ ] **Unused deps:** `cargo machete` (stable, fast) as primary; `cargo +nightly udeps` as a deeper
      secondary in a nightly job.
- [ ] **Unused code:** enable
      `#![warn(dead_code, unused_imports, unused_variables, unused_must_use)]` at each crate root;
      gate CI with `RUSTFLAGS="-D warnings"` for release profiles.
- [ ] **Dependency hygiene:** `cargo deny` for duplicate / yanked / banned crates.
- [ ] Decide whether to fail CI on findings or post as PR annotations only.

### TypeScript (`sdk/packages/node/iii/`, `sdk/packages/node/iii-browser/`,

`console/console-frontend/`, any other TS packages)

- [ ] **All-in-one:** `knip` — covers unused exports, unused files, unused deps in one config.
      Replaces `ts-prune` + `depcheck`.
- [ ] **Lint baseline:** ESLint `@typescript-eslint/no-unused-vars` and `no-unused-imports` (or
      `unused-imports` plugin).
- [ ] Add `knip.json` per package; commit a baseline ignore list for known-keep exports, then drive
      that list to zero.

### Python (`sdk/packages/python/iii/`)

- [ ] **Unused code:** `vulture` with a per-package allowlist for dynamic dispatch surfaces
      (decorators, plugin entrypoints).
- [ ] **Lint baseline:** `ruff` with `F401` (unused imports), `F841` (unused locals), `ARG` (unused
      arguments).
- [ ] **Unused deps:** `deptry`.

## 2. CI wiring

- [ ] Per-repo workflow that runs the language-appropriate checks on every PR.
- [ ] One scheduled (weekly) workflow that runs the deeper / nightly checks (`cargo +nightly udeps`,
      `knip --production`, `vulture --min-confidence 80`) and opens / updates a tracking issue with
      the diff.
- [ ] Output format: PR annotations + a single summary comment; no green-checkmark spam.
- [ ] Decide ownership: which team triages the weekly report.

## 3. Deprecated-but-still-shipped surfaces (index)

These are already tracked individually in their per-surface WORK files. Listed here so the dead-code
initiative can confirm each removal lands and the corresponding detector stays green afterward.

| Surface                                                    | Tracked in                                                                                                          | Status                                   |
| ---------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- | ---------------------------------------- |
| Adapter directories (`stream/state/queue/pubsub/cron`)     | [`WORK.engine.md`](./WORK.engine.md) §2                                                                             | pending removal                          |
| Adapter blocks in `engine/config*.yaml`                    | [`WORK.engine.md`](./WORK.engine.md) §2                                                                             | pending removal                          |
| `iii create` dispatcher entry                              | [`WORK.engine.md`](./WORK.engine.md) §1, [`WORK.cli.md`](./WORK.cli.md) §2, [`WORK.crates.md`](./WORK.crates.md) §1 | pending removal                          |
| `iii sandbox` top-level command                            | [`WORK.engine.md`](./WORK.engine.md) §1, [`WORK.cli.md`](./WORK.cli.md) §4                                          | pending removal                          |
| Forbidden SDK exports (channels / logger / OTel)           | [`WORK.sdk.md`](./WORK.sdk.md) §1                                                                                   | split: demote vs drop — see §5           |
| `register_function_with` (Rust SDK)                        | [`WORK.sdk.md`](./WORK.sdk.md) §1                                                                                   | demote (in cross-crate use) — see §5     |
| `registerTriggerType` / `unregisterTriggerType` (all SDKs) | [`WORK.sdk.md`](./WORK.sdk.md) §6                                                                                   | in skills use; decision pending — see §5 |
| `motia-tools` (if removed)                                 | [`WORK.crates.md`](./WORK.crates.md) §4                                                                             | decision pending                         |

- [ ] After each removal lands, confirm the relevant detector finds nothing flagging the removed
      surface (no leftover dead callers / re-exports / configs).

## 4. Repository-wide hygiene

- [ ] Orphaned-file sweep per repo (files not reachable from any build target, test, or doc
      include). Tooling: `knip` (TS), `cargo build --all-targets` + unreferenced-`mod.rs` audit
      (Rust), manual review (Python).
- [ ] Stale fixture / snapshot sweep (test fixtures referenced by no test).
- [ ] `README` / docs source files referenced from no nav and no link.

## 5. Current findings (2026-05-07 analysis)

Captured from a one-shot run of the Rust / TS / Python tools listed in §1, plus workspace-wide grep
where dead-code lints can't see cross-crate use. Methodology and limitations at the end.

### Decision framework

When a symbol is flagged as unused or removable:

1. **In active cross-crate / cross-package use, but not part of the user-facing API** → DEMOTE,
   don't remove. Two acceptable mechanisms:
   - **Option A — make internal:**
     - Rust: `pub(crate)` if the caller is same-crate; otherwise see Option B.
     - TypeScript: drop the `export` keyword; same-package callers import via the source path.
     - Python: prefix with `_`; same-package callers reference the underscore form.

   - **Option B — hide from docs and move to a clearly-named `__internal` module subspacing, plan
     future removal:**
     - Rust: `#[doc(hidden)] pub` + relocate under an `__internal` (or `unstable`, `internals`)
       module so the boundary is visible. Cross-crate callers update imports. Schedule removal once
       internal consumers migrate.
     - TypeScript: move into a `__internal/` subdirectory; remove from `index.ts`; add `@internal`
       JSDoc. Internal callers import from `iii/__internal/...`.
     - Python: move into an `_internal` submodule; drop from `__init__.py`.

2. **Public export with no callers anywhere in iii-temp** → Note that it is a public export but
   **completely unused internally**. Ask SDK consumers / external users whether they depend on it
   before dropping. Public-API removal is a breaking change.

3. **Engineering judgment** (test-only, ambiguous internal vs external use) → Per-item decision;
   record what the call sites are.

### A. Demote (in active cross-crate use; not user-facing)

Pick Option A or Option B per the decision framework. Either way: do not delete.

#### Rust SDK (`sdk/packages/rust/iii/src/lib.rs`)

- [ ] `register_function_with` (`iii.rs:951`)
  - Used by:
    `crates/iii-worker/src/sandbox_daemon/{fs/grep,fs/ls,fs/chmod,fs/write,fs/mkdir,fs/mv,fs/stat,fs/read,fs/rm,fs/sed,mod}.rs`
    (15+ sites), `skills/references/http-invoked-functions.rs`.
  - Note: an earlier instruction said "previously requested for removal" — that's inconsistent with
    current heavy use. Reconcile before any removal.
- [ ] `ChannelReader`, `ChannelWriter`, `StreamChannelRef`, `create_channel`, `ChannelDirection`,
      `ChannelItem`, `extract_channel_refs`, `is_channel_ref`
  - Used by: `crates/iii-worker/src/cli/sandbox.rs`,
    `crates/iii-worker/src/sandbox_daemon/fs/{write,read}.rs`.
- [ ] `OtelConfig`
  - Used by: `console/packages/console-rust/src/main.rs:100` (constructs
    `iii_sdk::OtelConfig { ... }`).

#### Python SDK (`sdk/packages/python/iii/src/iii/__init__.py`)

- [ ] `Logger`
  - Used by: `skills/references/*.py` (14+ files: trigger-conditions, queue-processing,
    cron-scheduling, realtime-streams, custom-triggers, observability, functions-and-triggers,
    http-endpoints, trigger-actions, dead-letter-queues, channels, state-management,
    state-reactions, http-invoked-functions).
  - Note: skills/references are user-facing examples; treat them as the same audience as external
    SDK consumers. If skills should not import `Logger`, migrate them first, then demote.
- [ ] `register_trigger_type` / `unregister_trigger_type`
  - Used by: `skills/references/custom-triggers.py:60,131`.
  - Update: previously marked **Consider removing** in `python-sdk.mdx`, but skills depend on them.
    See [`WORK.sdk.md`](./WORK.sdk.md) §6.

#### Rust SDK trigger-type method

- [ ] `register_trigger_type` (SDK method)
  - Used by: `skills/references/custom-triggers.rs:201,209`.
  - Note: same Consider-removing concern as Python — skills depend on it.

### B. Drop (public exports, completely unused internally — confirm with external consumers)

These items have the `pub` / `export` keyword but no callers anywhere in iii-temp. **Public exports
that are completely unused internally.** Before dropping, confirm with SDK consumers and downstream
users that they aren't relied on outside iii-temp; public-API removal is a breaking change. The
question to ask consumers: "is this of any use to you?"

#### Rust SDK

- [ ] `set_headers` (`sdk/packages/rust/iii/src/iii.rs:776`) — public method, zero callers in
      iii-temp.
- [ ] `set_metadata` (`sdk/packages/rust/iii/src/iii.rs:771`) — public method, zero callers in
      iii-temp.
- [ ] `set_otel_config` (`sdk/packages/rust/iii/src/iii.rs:781`) — public SDK method, zero callers
      (worker-internal `set_otel_config` in `engine/src/workers/observability/otel.rs:51` is a
      different function).
- [ ] `Logger` re-export — no external Rust users found.

#### Node SDK (`sdk/packages/node/iii/src/`)

- [ ] `TelemetryOptions` (`iii.ts`)
- [ ] `LoggerParams` (`logger.ts:9`)
- [ ] `Context` (`telemetry-system/index.ts:293`)
- [ ] `InternalFunctionHandler` (`types.ts:76`)
- [ ] `RemoteServiceFunctionData` (`types.ts:87`)
- [ ] `UnregisterTriggerTypeMessage` (`iii-types.ts:21`)
- [ ] `UnregisterTriggerMessage` (`iii-types.ts:26`)
- [ ] `UnregisterFunctionMessage` (`iii-types.ts:454`)

#### Browser SDK (`sdk/packages/node/iii-browser/src/`)

- [ ] `safeStringify` function (`utils.ts:7`)
- [ ] `TelemetryOptions` (`iii.ts:38`)
- [ ] `InternalFunctionHandler` (`types.ts:38`)
- [ ] `RemoteServiceFunctionData` (`types.ts:49`)
- [ ] `UnregisterTriggerTypeMessage` (`iii-types.ts:21`)
- [ ] `UnregisterTriggerMessage` (`iii-types.ts:26`)
- [ ] `UnregisterFunctionMessage` (`iii-types.ts:378`)
- [ ] `RegisterFunctionFormat` (`iii-types.ts:57`) — has `export` keyword but only used in declaring
      file (not imported by other files in the package). Either drop the `export` keyword (truly
      internal) or add to `index.ts` if intended as user-facing — see [`WORK.sdk.md`](./WORK.sdk.md)
      §2.

#### Engine self-update infrastructure

`engine/src/cli/{advisory,download,error,github,platform,registry,telemetry,update}.rs` contains
many functions flagged "never used" by cargo. Suggests `iii update` / advisory infrastructure may be
partially orphaned. Engineering review needed before drop OR wire-up. Items:

- [ ] `engine/src/cli/advisory.rs:14` constant `ADVISORIES_URL`; functions `fetch_advisories`,
      `print_advisory_warnings`.
- [ ] `engine/src/cli/download.rs` — functions `download_and_install`, `download_with_progress`,
      `verify_checksum`, `extract_binary`, `extract_from_targz`, `atomic_write_binary`.
- [ ] `engine/src/cli/error.rs:11` enum `IiiCliError` (entirely unused); multiple `StorageError`
      variants never constructed.
- [ ] `engine/src/cli/github.rs` — functions `build_client`, `fetch_latest_release`,
      `fetch_latest_release_simple`, `fetch_latest_release_by_prefix`.
- [ ] `engine/src/cli/platform.rs` — functions `state_file_path`, `checksum_asset_name`,
      `ensure_dirs`.
- [ ] `engine/src/cli/registry.rs:177` — function `all_binaries`.
- [ ] `engine/src/cli/telemetry.rs` — 5 send-related functions, 2 constants.
- [ ] `engine/src/cli/update.rs` — functions `check_for_updates`, `run_background_check`,
      `update_binary`, `self_update`, `update_all`.

Cross-reference: [`WORK.engine.md`](./WORK.engine.md) §1 (`iii update` confirmation) and
[`WORK.cli.md`](./WORK.cli.md) §3.

### C. Engineering judgment (test-only, ambiguous internal vs external)

#### Rust `get_connection_state` (`sdk/packages/rust/iii/src/iii.rs:1182`)

- Called only in 4 SDK tests (`tests/hold_process.rs`, `tests/rbac_workers.rs` 3x).
- No production callers in iii-temp.
- Decide: `pub(crate)` (test-only via cfg(test) re-export?), demote per §A, or keep public as a
  documented introspection API.

#### Python `get_connection_state` (`iii.py:678`)

- Vulture flagged unused intra-package.
- Decide: same options as Rust counterpart.

#### Browser `IIIReconnectionConfig`

- In real use internally (`iii-browser/src/iii.ts` lines 6, 62, 64, 79).
- NOT in `index.ts` exports.
- Users passing config can use structural typing, but cannot import the type for explicit
  annotation.
- Recommendation: re-export so users can write `Partial<IIIReconnectionConfig>` explicitly. See
  [`WORK.sdk.md`](./WORK.sdk.md) §2.

#### Python intra-package "unused" methods (low-confidence vulture findings)

- `register_service`, `create_channel`, `create_stream` — flagged by vulture but these are part of
  the canonical 9-method SDK shape per docs. External consumers may use them.
- Decide: confirm with SDK consumers; treat as low-confidence findings, not removal candidates.

#### Console-rust bridge `handle_*` functions

- `console/packages/console-rust/src/bridge/functions.rs` has many `handle_*` functions flagged
  "never used" plus `register_functions`.
- Likely registered with the engine via the bridge protocol at runtime; cargo's static analysis
  can't see the registration. Confirm before treating as dead.

### Methodology and limitations

Tools used:

```bash
# Rust — overrides any #[allow(dead_code)] decorators
cd iii-temp
RUSTFLAGS="--force-warn=dead_code --force-warn=unused_imports --force-warn=unused_variables" \
  cargo check --workspace --all-targets --all-features

# Node + Browser SDK
cd iii-temp/sdk/packages/node/iii && npx --yes knip --no-progress
cd iii-temp/sdk/packages/node/iii-browser && npx --yes knip --no-progress

# Python
cd iii-temp/sdk/packages/python/iii && uvx vulture src/
cd iii-temp/sdk/packages/python/iii && uvx ruff check --select F401,F841,F811 src/
```

Limitations:

- **Rust `dead_code` is intra-crate.** Public items in library crates are considered "used" by
  external crates regardless of actual callers. For SDK public methods, workspace-wide grep is the
  supplement.
- **Knip's "unused exports"** means "no other file in the package imports them." External SDK
  consumers (npm package users) are not visible to knip.
- **Vulture confidence** is appropriately low for pydantic field declarations, pydantic
  `model_config`, and class attributes used via reflection. Treat 60% findings as suggestions, not
  certainties.
- **Cross-crate / cross-package use** is invisible to all three tools individually. The §A demote
  list was assembled by manual workspace-wide grep.

## Notes / dependencies

- §1 (tooling) is independent per repo; can land in any order.
- §2 (CI) depends on §1 landing per repo first.
- §3 (deprecated index) is bookkeeping; depends on the linked WORK items, not the reverse.
- §4 (repo-wide hygiene) can run as a one-time pass before CI is wired so the baseline is clean.
- §5 (current findings) is a snapshot from 2026-05-07; re-run after each cleanup round and update.
