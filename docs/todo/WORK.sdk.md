# WORK — SDK packages

Remaining work on the four iii SDK packages (Node, Python, Rust, Browser). Sourced from
[`PROGRESS.md`](../PROGRESS.md) and [`PROGRESS-repo-checks.md`](../PROGRESS-repo-checks.md).

Repo locations:

- Node: `iii-temp/sdk/packages/node/iii/`
- Python: `iii-temp/sdk/packages/python/iii/`
- Rust: `iii-temp/sdk/packages/rust/iii/`
- Browser: `iii-temp/sdk/packages/node/iii-browser/`

Docs pages:

- `sdk-reference/node-sdk.mdx`
- `sdk-reference/python-sdk.mdx`
- `sdk-reference/rust-sdk.mdx`
- `sdk-reference/browser-sdk.mdx`

## 1. Strip forbidden surfaces from SDK packages

Per `project-rules/sdks.md` and `project-rules/workers.md`, logger / channels / OTel surfaces belong
in iii-observability and iii-worker-manager Worker Docs respectively. The docs pages are already
aligned with the rule (callouts in place; type entries stripped). The SDK packages still export
these symbols.

**Important — split per static analysis (2026-05-07):** several "drop these" items are actually in
active cross-crate / cross-package use. Don't remove them blindly. See
[`WORK.dead-code.md`](./WORK.dead-code.md) §5 for the full classification with file:line citations.
Two categories:

### A. Demote (in active cross-crate use, not user-facing)

Pick Option A (make internal) or Option B (hide from docs, move to `__internal` subspacing, plan
future removal) per the decision framework in [`WORK.dead-code.md`](./WORK.dead-code.md) §5. Per
language:

- Rust: `pub(crate)` if same-crate; otherwise `#[doc(hidden)] pub` and relocate to `__internal`
  module.
- TypeScript: drop `export` keyword OR move to `__internal/` subdir + `@internal` JSDoc.
- Python: prefix with `_` and remove from `__init__.py`.

Affected:

- **Rust SDK:** `register_function_with`, `ChannelReader`, `ChannelWriter`, `StreamChannelRef`,
  `create_channel`, `ChannelDirection`, `ChannelItem`, `extract_channel_refs`, `is_channel_ref`,
  `OtelConfig`. (Cross-crate consumers: `iii-worker` sandbox daemon, `console-rust`.)
- **Python SDK:** `Logger`, `register_trigger_type`, `unregister_trigger_type`. (Cross-package
  consumers: `skills/references/*.py`. May need to migrate skills first.)
- **Rust SDK:** `register_trigger_type` (SDK method). (Used by
  `skills/references/custom-triggers.rs`.)

### B. Drop (public exports, completely unused internally)

These have the `pub` / `export` keyword but no callers anywhere in iii-temp. **Confirm with SDK
consumers / external users that they aren't relied on outside iii-temp** before removal — public-API
removal is a breaking change. Question to ask: "is this of any use to you?"

- **Rust SDK:** `set_headers`, `set_metadata`, `set_otel_config`, `Logger` re-export,
  `ReconnectionConfig` (OTel), the entire `telemetry` module re-export (`init_otel`,
  `shutdown_otel`, `with_span`, baggage / traceparent helpers, OTel `SpanKind`, OTel `Status`,
  `execute_traced_request`).
- **Node SDK:** `TelemetryOptions`, `LoggerParams`, `Context`, `InternalFunctionHandler`,
  `RemoteServiceFunctionData`, `UnregisterTriggerTypeMessage`, `UnregisterTriggerMessage`,
  `UnregisterFunctionMessage`. Also `ChannelReader`, `ChannelWriter`, `Logger`, `StreamChannelRef`
  if no external consumers depend on them.
- **Python SDK:** `ChannelReader`, `ChannelWriter`, `StreamChannelRef`, `TelemetryOptions`,
  `OtelConfig` (after confirming no external use).
- **Browser SDK:** `safeStringify`, `TelemetryOptions`, `InternalFunctionHandler`,
  `RemoteServiceFunctionData`, `UnregisterTriggerTypeMessage`, `UnregisterTriggerMessage`,
  `UnregisterFunctionMessage`, `RegisterFunctionFormat` (only used in declaring file). Also
  `ChannelReader`, `ChannelWriter` if no external consumers depend on them.

After cleanup: re-run the analysis (per [`WORK.dead-code.md`](./WORK.dead-code.md) §5 methodology)
to confirm the docs and packages stay aligned.

## 2. Browser SDK exports decision

**Source:** `IIIReconnectionConfig` (defined in `iii-constants.ts`) and `RegisterFunctionFormat`
(defined in `iii-types.ts:57`) exist in source but are NOT re-exported from `index.ts`. Docs
currently strip both Types entries.

- [ ] Confirm intent with SDK authors:
  - **If intentional (types are internal):** no SDK change. Docs stay aligned.
  - **If accidental (`index.ts` should re-export):** add re-exports, then re-add Types entries to
    `sdk-reference/browser-sdk.mdx`.

## 3. Cross-SDK method confirmations

Methods present in some SDKs but not others. Static analysis on 2026-05-07 narrowed the findings
(see [`WORK.dead-code.md`](./WORK.dead-code.md) §5):

### `get_connection_state` (engineering judgment — §C)

- Python (`iii.py:678`): public; vulture flags unused intra-package.
- Rust (`iii.rs:1182`): public; called only in 4 SDK tests, no production callers.
- Node, Browser: not public.
- [ ] SDK author decision: `pub(crate)` for test-only / introspection-only, demote per §A of
      WORK.dead-code.md, or stub on docs and add Node/Browser parity.

### Rust-only `set_*` methods (confirmed unused — drop after consumer confirmation)

- `set_headers` (`iii.rs:776`) — zero callers in iii-temp.
- `set_metadata` (`iii.rs:771`) — zero callers in iii-temp.
- `set_otel_config` (`iii.rs:781`) — zero SDK-method callers (the same-named function in
  observability worker is internal to that worker).
- [ ] SDK author decision: confirm no external user dependence, then drop. See
      [`WORK.dead-code.md`](./WORK.dead-code.md) §5 — these are in category B (public exports,
      completely unused internally).

### Rust-only `register_function_with` (UPDATE 2026-05-07 — DEMOTE, do not remove)

- (`iii.rs:951`): heavily used by `crates/iii-worker/src/sandbox_daemon/*` (15+ sites) and
  `skills/references/http-invoked-functions.rs`.
- [ ] Demote per [`WORK.dead-code.md`](./WORK.dead-code.md) §5 §A. Earlier "remove" intent was
      inconsistent with current heavy use.

## 4. Cross-SDK type-list mismatches

Type lists across the four SDKs are not symmetric. Decide on the canonical set; align all four to
match (or add `_This differs from <other>_` reviewer notes per `sdks.md`).

### Source-verified differences (from `PROGRESS-repo-checks.md`)

- **Rust does not expose `IIIReconnectionConfig`** — reconnection internal, no user-facing engine-WS
  config. Genuine SDK gap vs Node, Python, Browser.
- **Rust does not expose `RegisterFunctionFormat`** — uses `schemars::JsonSchema` derive. Equivalent
  capability via different mechanism.
- **Rust does not expose `MessageType`** — internal to protocol module. Node exports it.
- **Rust adds vs Node/Python:** `IIIError`, `IIIConnectionState`, `HttpMethod`, `ChannelDirection`,
  `FunctionInfo`, `TriggerInfo`, `WorkerInfo`, `WorkerMetadata`, `RegisterServiceMessage`.
- **Python adds vs Node:** `TriggerActionEnqueue`, `TriggerActionVoid` (Rust expresses these as
  variants of a single `TriggerAction` enum).
- **Method differences:** Python has `trigger_async` and `shutdown_async`; Rust has `shutdown_async`
  and a natively-async `trigger`; Node has neither variant.

### Action items

- [ ] SDK authors: review the list above; decide which differences are intentional and which are
      gaps to close.
- [ ] Once decided, refresh `sdk-reference/*.mdx` Types lists to match the canonical set (with
      reviewer-note callouts for genuine cross-language differences).

## 5. Pre/post-append slots for auto-gen SDK reference

SDK reference pages will eventually be auto-generated from code. Auto-gen captures the surface
(methods, types, signatures) but not narrative content like "why use this SDK," migration guidance,
or when-to-pick-this-SDK.

- [ ] Design the slot mechanism. Options per `project-rules/sdks.md`:
  - Frontmatter slot.
  - Separate `_prefix.mdx` / `_suffix.mdx` partials.
  - A leading paragraph the generator preserves.
- [ ] Implement once the auto-gen tool is built (see [`WORK.auto-gen.md`](./WORK.auto-gen.md)).
- [ ] Populate slots:
  - Browser SDK prefix: "why the browser SDK over plain HTTP/REST" (already flagged in
    `sdk-reference/browser-sdk.mdx` as a `{/* TODO(skills/auto-gen) */}` marker — content sourced
    from iii-mono `docs/how-to/use-iii-in-the-browser.mdx`).
  - Other SDKs: motivational/positioning prose TBD.

## 6. `registerTriggerType` / `unregisterTriggerType` — keep or remove

**Source:** PROGRESS.md SDK heavy-stubs decision log. Each SDK page (`node-sdk.mdx`,
`python-sdk.mdx`, `rust-sdk.mdx`, `browser-sdk.mdx`) marks both methods with **Consider removing**.
PROGRESS.md notes: "If trigger-type registration becomes worker-author / worker-internal, [the
create-custom-trigger-type how-to] belongs outside ideal-docs." The how-to was dropped on that
basis.

**Update 2026-05-07 (static analysis):** these methods are in active use by skill references — the
**Consider removing** markers may be wrong:

- Rust SDK `register_trigger_type` — `skills/references/custom-triggers.rs:201,209`.
- Python SDK `register_trigger_type` — `skills/references/custom-triggers.py:60,131`.

The wire-level `Unregister*Message` types ARE confirmed unused by knip (Node and Browser). So:

- [ ] SDK author decision: keep the methods (skills depend on them) but consider whether the
      wire-level `Unregister*Message` types are actually invoked or are dead remnants — would mean
      unregistration is a docs / skills concept that doesn't roundtrip to the engine.
- [ ] If keeping: remove the **Consider removing** markers from all four SDK pages.
- [ ] If removing: migrate `skills/references/custom-triggers.{rs,py}` first; only then drop the SDK
      methods.

See [`WORK.dead-code.md`](./WORK.dead-code.md) §5 §A (skill consumers) and §B (unused wire types)
for full classification.

## 7. Cross-SDK alignment guardrails

- [ ] Once the surface stabilizes, add a CI check that fails if `sdk-reference/*.mdx` references a
      method/type that doesn't appear in the canonical set (and vice versa). Prevents drift after
      the next round of authoring.

## 8. Inline markup conventions to preserve

Per PROGRESS.md, the SDK pages use these conventions; preserve them across authoring rounds and
resolve only when the underlying question is resolved:

- `**Consider removing**` — flagged surfaces that may leave the public API (currently on
  `registerTriggerType` / `unregisterTriggerType` — see §6).
- `_Confused on this one_` / `_This differs from node, is this okay?_` — italic reviewer notes
  embedded as questions for SDK authors.
- `<Note>` callouts at the top pointing to the worker that owns a stripped surface.
- `{/* TODO(skills/auto-gen): ... */}` — MDX comment markers for content that needs hand-authored
  prefix/suffix once auto-gen lands (see §5).

## 9. Dead code detection

See [`WORK.dead-code.md`](./WORK.dead-code.md) for the cross-cutting initiative. SDK-specific items:

- [ ] **Node + Browser:** add `knip` per package with a baseline ignore list; drive the list to
      zero. Add `@typescript-eslint/no-unused-vars` to ESLint.
- [ ] **Python:** add `vulture` and `ruff` (`F401`, `F841`, `ARG`); add `deptry` for unused
      dependencies in `pyproject.toml`.
- [ ] **Rust:** enable `#![warn(dead_code, unused_imports, unused_variables)]` at each SDK crate
      root; add `cargo machete` to CI.
- [ ] After §1 (forbidden-surface cleanup) lands, confirm no internal-only channel / logger / OTel
      symbols linger as private dead code.
- [ ] After §6 (`registerTriggerType` decision) lands, run detectors to remove any resulting dead
      helpers / message types.

## Notes / dependencies

- Forbidden-surface cleanup is independent and can ship anytime.
- Auto-gen slots depend on the auto-gen tooling design ([`WORK.auto-gen.md`](./WORK.auto-gen.md)).
- The "Why this SDK" framing for browser-sdk has no auto-gen home until slots land.
- Python+Rust `get_connection_state` and Rust `set_*` methods are blocked on SDK-author
  confirmation, not docs work.
