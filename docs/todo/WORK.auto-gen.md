# WORK — Auto-generation tooling

Auto-gen tooling for the config reference and SDK reference docs. Sourced from
[`PROGRESS.md`](../PROGRESS.md) hanging pieces and
[`PROGRESS-repo-checks.md`](../PROGRESS-repo-checks.md).

This is a separate engineering project from the ideal-docs migration. PROGRESS.md says:
"Don't attempt during this migration pass." It's queued.

## 1. Config reference auto-gen

### Background
Pre-Mintlify, `how-to/configure-engine.mdx` was generated from a commented
`iii-config.yaml` by a build-time parser + React component:
- `docs/content/how-to/iii-config.yaml`
- `docs/src/lib/config-parser.ts`
- `docs/src/lib/config-toc.ts`
- `docs/src/lib/components/ConfigReference.tsx`

This was lost in the `feat: mintlify` migration (commit `0f925fd2` deleted; `79bdd94d`
introduced Mintlify).

### Goal
Regenerate the configuration reference and surface it inside `using-iii/engine.mdx` (or
wherever fits best after final structure).

### Implementation sketch
- [ ] Author a fresh commented `config.yaml` colocated with the engine, using current
  naming (`name: iii-http`, `name: iii-stream`, etc.) and **no deprecated adapter
  sections**. (Per `project-rules/general.md`.)
- [ ] Write a generator that emits a Mintlify MDX fragment. Two viable mechanisms:
  - Pre-build script that commits `using-iii/engine.config-reference.generated.mdx`.
  - `_snippets/config-reference.mdx` consumed via `<Snippet file="..." />`.
- [ ] Transclude into the host page (`using-iii/engine.mdx` or new dedicated page).

### Open question (engineering decision)
- [ ] **Where does this content live — ideal-docs or Worker Docs?** With "everything is
  a worker," what used to read as engine config is largely per-worker config (HTTP
  host/port/CORS → iii-http; stream auth_function → iii-stream; etc.). Decide:
  - One cross-cutting page in ideal-docs (`using-iii/engine.mdx`), or
  - Split per Worker Docs surface (each worker's docs has its own config-reference
    section), or
  - Both (high-level cross-cutting + per-worker detail).

### Pre-conditions
- [ ] Adapter cleanup in engine code (see [`WORK.engine.md`](./WORK.engine.md) §2). The
  fresh commented `config.yaml` source should not reintroduce adapter blocks.
- [ ] `iii-config.yaml` → `config.yaml` naming normalization (see
  [`WORK.engine.md`](./WORK.engine.md) §3) so the source-of-truth file is unambiguously
  named.

## 2. SDK reference auto-gen

### Background
SDK reference pages (`sdk-reference/{node,python,rust,browser}-sdk.mdx`) will eventually
be auto-generated from the SDK packages' source. Auto-gen captures the surface (methods,
types, signatures) but not narrative content like "why use this SDK," migration
guidance, or when-to-pick-this-SDK.

### Goal
Auto-generate the surface portion of each SDK reference page; preserve hand-written
narrative via a slot mechanism.

### Implementation sketch
- [ ] Build a per-language extractor that reads the SDK source and emits a Mintlify MDX
  fragment with the methods + types listing in the existing page format.
- [ ] Define the slot mechanism for hand-written narrative. Options per
  `project-rules/sdks.md`:
  - **Frontmatter slot:** generator preserves a designated frontmatter field that
    renders above/below the auto-gen content.
  - **Separate partials:** `_prefix.mdx` / `_suffix.mdx` files alongside each SDK page;
    generator transcludes them.
  - **Preserved leading paragraph:** generator detects and preserves any prose above the
    first `##` heading.
- [ ] Pick one and implement it.
- [ ] Populate the slots:
  - Browser SDK prefix: "why the browser SDK over plain HTTP/REST" (currently flagged in
    `sdk-reference/browser-sdk.mdx` as a `{/* TODO(skills/auto-gen) */}` marker; source
    content lived in iii-mono `docs/how-to/use-iii-in-the-browser.mdx`).
  - Other SDKs: motivational/positioning prose TBD.
- [ ] CI: run the generator on every commit; fail if hand-edited surface portions
  drift from the auto-gen output.

### Pre-conditions
- [ ] SDK forbidden-surface cleanup (see [`WORK.sdk.md`](./WORK.sdk.md) §1) so the
  generator doesn't include logger / channels / OTel exports.
- [ ] Browser SDK exports decision (see [`WORK.sdk.md`](./WORK.sdk.md) §2) so
  `IIIReconnectionConfig` / `RegisterFunctionFormat` either are or aren't in the
  generator's source set.
- [ ] Rust SDK methods decisions (see [`WORK.sdk.md`](./WORK.sdk.md) §3) so
  `set_headers` / `set_metadata` / `set_otel_config` either are or aren't in the
  generator's source set.

## 3. Engine SDK reference auto-gen

`sdk-reference/engine-sdk.mdx` is unique — it's a worker's surface (iii-engine-functions)
exposed as if it were a separate SDK. The audit kept the friendly "Engine SDK" alias.

- [ ] Decide whether `engine-sdk.mdx` is auto-generated from the iii-engine-functions
  worker definition, hand-authored, or absorbed into iii-engine-functions Worker Docs.
- [ ] If auto-generated: the same slot mechanism (§2) applies; the source is the worker
  registration code rather than an SDK package.
- [ ] If absorbed into Worker Docs: the friendly "Engine SDK" page either redirects or
  becomes a thin alias page. See [`WORK.engine.md`](./WORK.engine.md) §4 (engineering
  naming review).

## 4. Cross-cutting design considerations

### Dependency on the surface stabilizing
- Auto-gen is brittle when the surface is in flux. Sequence after:
  - SDK forbidden-surface cleanup ([`WORK.sdk.md`](./WORK.sdk.md) §1).
  - SDK cross-language alignment ([`WORK.sdk.md`](./WORK.sdk.md) §4).
  - Engine adapter cleanup ([`WORK.engine.md`](./WORK.engine.md) §2).
  - Engine naming reviews ([`WORK.engine.md`](./WORK.engine.md) §4) so worker / function
    names are stable.

### Inline markup convention
- Per PROGRESS.md, `{/* TODO(skills/auto-gen): ... */}` MDX comment markers flag spots
  the auto-gen needs to either preserve, replace, or interpret as slot anchors.
- [ ] Generator should recognize these markers as instructions, not as random comments.

### Versioning
- [ ] Decide whether the auto-gen output is versioned per-SDK release, or whether the
  docs always reflect `main` (with a banner for older releases). Affects publish
  pipeline.

## Notes / dependencies

- §1 (config reference) depends on engine cleanup ([`WORK.engine.md`](./WORK.engine.md)
  §2, §3).
- §2 (SDK reference) depends on SDK cleanup ([`WORK.sdk.md`](./WORK.sdk.md) §1, §2, §3).
- §3 (engine SDK) depends on engine naming review ([`WORK.engine.md`](./WORK.engine.md)
  §4).
- All three sub-projects share infrastructure (Mintlify build hooks, MDX fragment
  emission); building one likely yields scaffolding for the others.
- The user explicitly said in PROGRESS.md: "Don't attempt during this migration pass."
  Queue this for the post-outline engineering phase.
