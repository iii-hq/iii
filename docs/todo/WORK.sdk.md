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

Per `project-rules/sdks.md` and `project-rules/workers.md`, logger / channels / OTel
surfaces belong in iii-observability and iii-worker-manager Worker Docs respectively. The
docs pages are already aligned with the rule (callouts in place; type entries stripped).
The SDK packages still export these symbols. Punch list (per package):

### Node SDK (`sdk/packages/node/iii/src/index.ts`)
- [ ] Drop `ChannelReader`
- [ ] Drop `ChannelWriter`
- [ ] Drop `Logger`
- [ ] Drop `StreamChannelRef`
- [ ] Drop `otel?: TelemetryOptions` from `InitOptions`

### Python SDK (`sdk/packages/python/iii/src/iii/__init__.py`)
- [ ] Drop `ChannelReader`
- [ ] Drop `ChannelWriter`
- [ ] Drop `Logger`
- [ ] Drop `StreamChannelRef`
- [ ] Drop `OtelConfig`
- [ ] Drop `TelemetryOptions`

### Rust SDK (`sdk/packages/rust/iii/src/lib.rs:114-127`)
- [ ] Drop the entire `telemetry` module re-export (`init_otel`, `shutdown_otel`,
  `with_span`, all baggage / traceparent helpers, OTel `SpanKind`, OTel `Status`,
  `execute_traced_request`, etc.)
- [ ] Drop `ChannelDirection`
- [ ] Drop `ChannelItem`
- [ ] Drop `ChannelReader`
- [ ] Drop `ChannelWriter`
- [ ] Drop `StreamChannelRef`
- [ ] Drop `extract_channel_refs`
- [ ] Drop `is_channel_ref`
- [ ] Drop `Logger`
- [ ] Drop `OtelConfig`
- [ ] Drop `ReconnectionConfig` (the OTel one)
- [ ] Remove `register_function_with` (`iii.rs:951`) — previously flagged for removal

### Browser SDK (`sdk/packages/node/iii-browser/src/index.ts`)
- [ ] Drop `ChannelReader`
- [ ] Drop `ChannelWriter`
- [ ] Drop `TelemetryOptions` from `InitOptions`

After cleanup: re-run a survey against `sdk-reference/*.mdx` to confirm the docs and
packages stay aligned.

## 2. Browser SDK exports decision

**Source:** `IIIReconnectionConfig` (defined in `iii-constants.ts`) and
`RegisterFunctionFormat` (defined in `iii-types.ts:57`) exist in source but are NOT
re-exported from `index.ts`. Docs currently strip both Types entries.

- [ ] Confirm intent with SDK authors:
  - **If intentional (types are internal):** no SDK change. Docs stay aligned.
  - **If accidental (`index.ts` should re-export):** add re-exports, then re-add Types
    entries to `sdk-reference/browser-sdk.mdx`.

## 3. Cross-SDK method confirmations

Methods present in some SDKs but not others. Decide whether each is a primary surface
(stub on docs + add to other SDKs for parity) or an internal helper (keep undocumented;
mark `pub(crate)` / non-public in code).

### `get_connection_state`
- Python (`iii.py:678`): public
- Rust (`iii.rs:1182`): public
- Node, Browser: not public (Node has internal `IIIConnectionState` tracking only)
- [ ] SDK author decision: primary surface or internal? If primary, add to all four.

### Rust-only methods
- `set_headers` (`iii.rs:776`)
- `set_metadata` (`iii.rs:771`)
- `set_otel_config` (`iii.rs:781`) — additionally subject to the observability-stripping
  rule; if kept public, joins the forbidden-surface cleanup list.
- [ ] SDK author decision: primary surface or internal? If primary, stub on `rust-sdk.mdx`
  with reviewer notes and consider Node / Python parity.

## 4. Cross-SDK type-list mismatches

Type lists across the four SDKs are not symmetric. Decide on the canonical set; align all
four to match (or add `_This differs from <other>_` reviewer notes per `sdks.md`).

### Source-verified differences (from `PROGRESS-repo-checks.md`)
- **Rust does not expose `IIIReconnectionConfig`** — reconnection internal, no
  user-facing engine-WS config. Genuine SDK gap vs Node, Python, Browser.
- **Rust does not expose `RegisterFunctionFormat`** — uses `schemars::JsonSchema` derive.
  Equivalent capability via different mechanism.
- **Rust does not expose `MessageType`** — internal to protocol module. Node exports it.
- **Rust adds vs Node/Python:** `IIIError`, `IIIConnectionState`, `HttpMethod`,
  `ChannelDirection`, `FunctionInfo`, `TriggerInfo`, `WorkerInfo`, `WorkerMetadata`,
  `RegisterServiceMessage`.
- **Python adds vs Node:** `TriggerActionEnqueue`, `TriggerActionVoid` (Rust expresses
  these as variants of a single `TriggerAction` enum).
- **Method differences:** Python has `trigger_async` and `shutdown_async`; Rust has
  `shutdown_async` and a natively-async `trigger`; Node has neither variant.

### Action items
- [ ] SDK authors: review the list above; decide which differences are intentional and
  which are gaps to close.
- [ ] Once decided, refresh `sdk-reference/*.mdx` Types lists to match the canonical set
  (with reviewer-note callouts for genuine cross-language differences).

## 5. Pre/post-append slots for auto-gen SDK reference

SDK reference pages will eventually be auto-generated from code. Auto-gen captures the
surface (methods, types, signatures) but not narrative content like "why use this SDK,"
migration guidance, or when-to-pick-this-SDK.

- [ ] Design the slot mechanism. Options per `project-rules/sdks.md`:
  - Frontmatter slot.
  - Separate `_prefix.mdx` / `_suffix.mdx` partials.
  - A leading paragraph the generator preserves.
- [ ] Implement once the auto-gen tool is built (see [`WORK.auto-gen.md`](./WORK.auto-gen.md)).
- [ ] Populate slots:
  - Browser SDK prefix: "why the browser SDK over plain HTTP/REST" (already flagged in
    `sdk-reference/browser-sdk.mdx` as a `{/* TODO(skills/auto-gen) */}` marker — content
    sourced from iii-mono `docs/how-to/use-iii-in-the-browser.mdx`).
  - Other SDKs: motivational/positioning prose TBD.

## 6. `registerTriggerType` / `unregisterTriggerType` — keep or remove

**Source:** PROGRESS.md SDK heavy-stubs decision log. Each SDK page (`node-sdk.mdx`,
`python-sdk.mdx`, `rust-sdk.mdx`, `browser-sdk.mdx`) marks both methods with
**Consider removing**. PROGRESS.md notes: "If trigger-type registration becomes
worker-author / worker-internal, [the create-custom-trigger-type how-to] belongs outside
ideal-docs." The how-to was dropped on that basis.

- [ ] SDK author decision: keep these as user-facing methods, or remove from public
  exports and treat trigger-type registration as a worker-internal concern?
- [ ] If removed: drop the `## registerTriggerType` and `## unregisterTriggerType`
  stubs from all four SDK reference pages and the corresponding `RegisterTriggerTypeInput`
  / `RegisterTriggerTypeMessage` types.
- [ ] If kept: remove the **Consider removing** markers; author the stubs.

## 7. Cross-SDK alignment guardrails

- [ ] Once the surface stabilizes, add a CI check that fails if `sdk-reference/*.mdx`
  references a method/type that doesn't appear in the canonical set (and vice versa).
  Prevents drift after the next round of authoring.

## 8. Inline markup conventions to preserve

Per PROGRESS.md, the SDK pages use these conventions; preserve them across authoring
rounds and resolve only when the underlying question is resolved:

- `**Consider removing**` — flagged surfaces that may leave the public API (currently
  on `registerTriggerType` / `unregisterTriggerType` — see §6).
- `_Confused on this one_` / `_This differs from node, is this okay?_` — italic
  reviewer notes embedded as questions for SDK authors.
- `<Note>` callouts at the top pointing to the worker that owns a stripped surface.
- `{/* TODO(skills/auto-gen): ... */}` — MDX comment markers for content that needs
  hand-authored prefix/suffix once auto-gen lands (see §5).

## Notes / dependencies

- Forbidden-surface cleanup is independent and can ship anytime.
- Auto-gen slots depend on the auto-gen tooling design ([`WORK.auto-gen.md`](./WORK.auto-gen.md)).
- The "Why this SDK" framing for browser-sdk has no auto-gen home until slots land.
- Python+Rust `get_connection_state` and Rust `set_*` methods are blocked on SDK-author
  confirmation, not docs work.
