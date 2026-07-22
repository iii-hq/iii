# Authoring injectable UI — the worker side

What a worker author actually writes: one content function, one trigger
registration, one `@iii-dev/console-build` invocation. This document is the
would-be SKILL/SOP material, kept in the spec until the feature ships.

## The three pieces

Using the brief's example — the `state` worker adding a page at
`state/page.js`:

```text
workers/state/
  ui/
    page.tsx          # ordinary React, imports from 'react' and '@iii/console'
    styles.css        # optional Tailwind entry ([Styling & Tailwind](#styling--tailwind))
  src/ (or index.ts)  # worker code: content function + trigger registration
```

### 1. The component (`ui/page.tsx`)

```tsx
import { useState, useEffect } from 'react'
import type { Host } from '@iii/console'

function StatesPage({ host }: { host: Host }) {
  const [groups, setGroups] = useState<string[]>([])
  useEffect(() => {
    host.iii.trigger<{ groups: string[] }>('state::list-groups', {})
      .then(r => setGroups(r.groups))
  }, [])
  const { EmptyState } = host.components
  if (!groups.length) return <EmptyState title="no state groups yet" />
  return <ul>{groups.map(g => <li key={g}>{g}</li>)}</ul>
}

export default function setup(host: Host) {
  host.pages.register({
    id: 'state-states',
    title: 'States',
    render: () => <StatesPage host={host} />,
  })
  // host-tracked registration — no manual cleanup needed
}
```

### 2. The build

The recommended builder is **`@iii-dev/console-build`** — a thin CLI (plus a
Node library used in [the dev loop](#the-dev-loop-hot-reload-from-the-authors-chair))
that owns the whole pipeline, so the externals/scoping contract is not
something an author can get subtly wrong:

```bash
npx @iii-dev/console-build --worker state
# ui/*.tsx → dist/ui/*.js · ui/styles.css → dist/ui/styles.css (scoped)
```

What it does:

- **JS**: esbuild underneath — `--bundle --format=esm --jsx=automatic`, the
  five shared specifiers external (they resolve at runtime through the
  console's import map,
  [slots-and-api.md § boot contract](slots-and-api.md#sharing-one-react-the-boot-contract)).
  Everything else is bundled in.
- **CSS**: Tailwind v4 against the `@iii/console/tailwind.css` preset, then
  the scoping post-pass under `[data-iii-ui="<worker>"]` — see
  [Styling & Tailwind](#styling--tailwind).
- **Lints (build failures, not warnings)**: a bundled React copy (a forgotten
  external otherwise surfaces at runtime as a cryptic "Invalid hook call" in
  the console), and CSS the scoper cannot contain (`@font-face`,
  unrewritable selectors).

The tool is convenience, not contract — the wire is bytes. The equivalent
raw esbuild invocation, for authors who bring their own pipeline:

```bash
esbuild ui/page.tsx --bundle --format=esm --outfile=dist/ui/page.js \
  --external:react --external:react-dom --external:react-dom/client \
  --external:react/jsx-runtime --external:@iii/console \
  --jsx=automatic
```

Two raw-pipeline footguns the tool otherwise absorbs: `--external:react-dom`
also externalizes subpaths (`react-dom/server`, …) — and only the five listed
specifiers exist in the import map, so a transitive dependency importing any
other bare react-family specifier fails at `import()` time, not at build
time. And a forgotten external means a second React: the component's hooks
resolve against the bundled copy's never-installed dispatcher — the "Invalid
hook call" failure named above, with nothing pointing at the cause.

Everything **else** the component needs gets bundled in. Keep output well
under the console's 8 MiB per-asset cap
([injection-protocol.md § fetch policy](injection-protocol.md#fetch-policy));
a slot component should be tens of KiB.

### 3. Registration (Node SDK shown; the wire contract is SDK-agnostic)

```ts
import { readFile } from 'node:fs/promises'

// the content function — one per worker, serves all its assets
iii.registerFunction('state::ui-content', async ({ path }: { path: string }) => {
  const file = ASSETS[path]                      // e.g. { 'state/page.js': 'dist/ui/page.js' }
  if (!file) throw new Error(`unknown ui asset: ${path}`)
  return { content: await readFile(file, 'utf8') }
})

// one trigger per asset — registration IS deployment
const trigger = iii.registerTrigger({
  type: 'console:script',                        // or 'console:style'
  function_id: 'state::ui-content',
  config: { path: 'state/page.js' },
})
// trigger.unregister() removes the asset; worker disconnect does it implicitly
```

Node surface: `registerTrigger` returns `Trigger { unregister() }` and mints
the trigger id internally (`iii/sdk/packages/node/iii/src/iii.ts:265-285`);
`registerFunction` handlers are ordinary typed functions. The Rust equivalent
is `iii.register_trigger(RegisterTriggerInput { trigger_type, function_id,
config, metadata })` (`iii/sdk/packages/rust/iii/src/protocol.rs:201-214`)
plus a `RegisterFunction::new_async` content function — see the console's own
`console::status` for the idiom (`workers/console/src/functions/mod.rs:20-48`).

**Use the SDK Message path, not `engine::register_trigger`** — the reasons are
lifecycle, not style
([injection-protocol.md § lifecycle](injection-protocol.md#lifecycle-matrix)).

## The dev loop (hot reload from the author's chair)

```bash
npx @iii-dev/console-build --worker state --watch
```

Building is the tool's job; **registration stays in the worker process** —
deliberately. Message-path triggers die with the connection that registered
them ([injection-protocol.md § lifecycle](injection-protocol.md#lifecycle-matrix));
a build tool that registered on the worker's behalf would tie asset lifetime
to the watcher instead of the worker. So:

- **Node workers** embed the watcher and hand it their registrar — the
  library implements the full re-register discipline below:

  ```ts
  import { watchUi } from '@iii-dev/console-build'

  if (dev) watchUi({
    worker: 'state',
    register: (path, type) => iii.registerTrigger({
      type,                       // 'console:script' | 'console:style'
      function_id: 'state::ui-content',
      config: { path },
    }),
  })
  ```

- **Rust (and other) workers** run the CLI standalone and watch their own
  `dist/ui/` output for changes (`notify` crate, a few lines mirroring the
  Node watcher), re-registering on change.

The loop each rebuild runs:

1. Save `page.tsx` → `console-build` rebuilds `dist/ui/page.js`.
2. The watch hook re-registers the path, **then unregisters the previous
   handle** (`watchUi` does exactly this; the snippet is what any
   integration must do):

   ```ts
   const next = iii.registerTrigger({ type: 'console:script', function_id: 'state::ui-content',
                                      config: { path: 'state/page.js' } })
   prev?.unregister()
   prev = next
   ```

   Register-first avoids a zero-trigger window (a flash-dispose in tabs); the
   trailing unregister is a designed no-op beyond the SDK — the console
   already pruned the superseded engine row, so the engine answers
   `removed: false` and nothing reaches the console.
3. Console worker re-fetches, hash changes, every open tab hot-swaps the
   component in place — no console rebuild, no tab refresh.

The `unregister()` in step 2 is contract, not tidiness: console-side pruning
reaches only the **engine** registry, while both SDKs keep every registration
in an in-memory map that is replayed wholesale on reconnect (Node
`iii/sdk/packages/node/iii/src/iii.ts:273,783-790`; Rust
`collect_registrations`, `iii/sdk/packages/rust/iii/src/iii.rs:1550-1566`),
and only `unregister()` removes an entry. Skip it and every reconnect replays
your entire rebuild history — harmless (the console converges) but churny:
n register frames, n content fetches, n−1 supersede warns.

Ordering never matters: register before the console worker is up and the
engine parks the intent and delivers it when the console arrives
(`RegisterTriggerOutcome::Deferred`, `iii/engine/src/trigger.rs:40-49`);
restart the console and it replays; restart the engine and the SDK re-registers
on reconnect.

## Styling & Tailwind

Three cooperating mechanisms style injected UI; a worker uses any mix:

1. **Console tokens** — the design system is plain CSS custom properties
   flipped on `html[data-theme]` (`workers/console/web/src/index.css:12-74`);
   `var(--color-accent)` in any injected style follows light/dark for free.
2. **Pre-styled host components** — `host.components` arrives styled by the
   console's own CSS.
3. **The worker's own `style` asset** — typically compiled Tailwind; the rest
   of this section is the contract that makes that safe.

### Why raw Tailwind output cannot be injected as-is

The console is Tailwind v4 (`@import "tailwindcss"`,
`workers/console/web/src/index.css:10`), so **all** console CSS lives inside
`@layer theme/base/components/utilities`. Three consequences (CSS cascade +
Tailwind v4 emission behavior; model knowledge, 2026 — re-verify exact
Tailwind mechanics at implementation time):

- **Unlayered beats layered.** Any unlayered rule in a later-loaded injected
  sheet outranks every console rule of equal specificity — a stray
  `button { … }` restyles the whole console silently.
- **Utility names are document-global.** An unprefixed worker build defines
  `.flex`, `.text-sm`, … in the same document-merged `utilities` layer as
  the console's copies; later source order wins document-wide, and "later"
  is registration order — nondeterministic across reconnects once N workers
  ship Tailwind.
- **Used theme variables are emitted at `:root`.** A worker using `font-sans`
  emits Tailwind's default `--font-sans` at `:root`; the later link wins and
  the console's Geist (`index.css:13`) flips to the system font — the
  flagship silent failure.

### The scope contract (how the problems disappear)

The host mounts every injected render inside
`<div data-iii-ui="<worker>" style="display:contents">`
([slots-and-api.md § The scope wrapper](slots-and-api.md#the-scope-wrapper)),
and `@iii-dev/console-build` compiles the worker's CSS so **every rule sits
under that attribute**:

```css
/* .flex from worker `state` compiles to: */
[data-iii-ui="state"] .flex { display: flex }
/* :root / html / body selectors are rewritten to the scope root
   (custom properties then inherit down the worker's subtree only): */
[data-iii-ui="state"] { --spacing: 0.25rem }
```

Console elements are never inside the wrapper, so nothing leaks out; inside
the subtree the worker's definitions win by attribute specificity —
deterministic regardless of load order, coexisting with any number of other
workers' builds. `@keyframes` names (inherently global) are namespaced with
the worker prefix at build time; `@font-face` cannot be scoped and fails the
build (bundle fonts as data: URLs or rely on the console's).

### The preset: `@iii/console/tailwind.css`

Ships in the `@iii/console` package, **generated at console build time
alongside the `/vendor` shims** (same rationale: hand-maintained lists
drift; a CI assertion keeps the token map ⊇ `index.css`'s `@theme`). A
worker's entire Tailwind entry:

```css
/* ui/styles.css */
@import "@iii/console/tailwind.css";
@source "./";
```

Contents:

- **Utilities only — never Tailwind's preflight**, which would re-reset the
  console document from a later-loaded sheet (version skew between the
  console's Tailwind and the worker's becomes console-wide visual drift).
- **`@custom-variant dark`** bound to `html[data-theme="dark"]`, so `dark:`
  follows the console's toggle rather than the v4 default
  `prefers-color-scheme`. (The console's own `Dialog.tsx:15,21` currently
  has this exact mismatch — `dark:bg-bg-dark` against a token that doesn't
  exist; fixing it rides along with generating the preset.)
- **`@theme inline`** mapping the console tokens — `bg-accent`,
  `text-ink-faint`, `bg-panel`, … compile to `var(--color-*)` references
  (nothing re-emitted at `:root`), so worker UI themes with the console
  automatically.
- An optional companion `@iii/console/tailwind-reset.css` — a minimal
  box-model reset scoped to the wrapper, for authors who miss preflight.

### What stays not-quite-normal (the honest residue)

- **No preflight inside slots** — the console document's base styles apply
  to injected markup. Usually desirable (it's why injected UI looks native);
  the scoped reset above is the opt-out.
- **Worker-owned portals need the attribute.** `host.components.Dialog` is
  pre-stamped by the host ([slots-and-api.md § The scope
  wrapper](slots-and-api.md#the-scope-wrapper)); DOM a worker portals to
  `document.body` itself must carry `data-iii-ui="<worker>"` on its root or
  its scoped styles go dead there.
- **Not a security boundary.** Scoping is compile-side hygiene; a hand-built
  sheet can still ship unscoped selectors. The console's answer is a
  warn-only lint on `style` assets
  ([injection-protocol.md § Style lint](injection-protocol.md#style-lint-warn-only)),
  consistent with the trust model.

## Conventions

- **Paths**: `<worker>/<name>.<ext>`, lowercase kebab — `state/page.js`,
  `state/theme.css`. The prefix is the only human-visible attribution
  ([injection-protocol.md § config contract](injection-protocol.md#the-config-contract)).
- **Function ids**: kebab-case `<worker>::<verb>` per
  `workers/docs/sops/binary-worker.md` — `state::ui-content`. Typed
  request/response structs are mandatory for Rust workers (golden schema
  tests); the content function is public wire surface like any other.
- **One content function, many assets.** Dispatch on `path` inside it.
- **Style assets** use the console's CSS custom properties
  (`var(--color-accent)` etc., `workers/console/web/src/index.css:12-74`) and
  never hardcode theme colors — dark mode is a variable flip on
  `html[data-theme]`. Tailwind, scoping, and CSS hygiene generally have
  their own contract: [Styling & Tailwind](#styling--tailwind).
- **Component reuse is intentionally shallow.** Prefer `host.components` (the
  curated library) for chrome; for anything richer, *copy the pattern into
  your worker*. Small render components duplicated across workers is the
  accepted cost — workers version and deploy independently, and a shared
  "extension components" package would couple their release cycles to the
  console's. The `@iii/console` surface is deliberately the only versioned
  contract. (Expect the same little status-pill in three workers; that's
  fine.)
- **Multiple assets per worker** are normal: a page script, a composer script,
  a stylesheet — three triggers, one content function.

## Compatibility & versioning

- The **contract surface** is: the trigger config schema, the content-function
  shape, the `setup(host)` signature, the `Host`/`ConsoleApi` types, the four
  slot prop interfaces, the five import-map specifiers, the `data-iii-ui`
  scope attribute (name and first-path-segment value), and the
  `@iii/console/tailwind.css` token map. These version with
  the console worker; additive growth only, breaking changes get a major
  console release and a migration note.
- An `@iii/console` **types package** (npm, types-only + the documented
  interfaces) gives authors compile-time checking; at runtime the import map
  supplies the real module. The same package ships the generated
  `tailwind.css` preset and the optional scoped reset. Publishing it — and
  `@iii-dev/console-build` — is part of shipping this spec.
- React major upgrades in the console change the shim export list
  (`/vendor/react.js`) — additive for minors; a React major bump is a
  console major and announced as such. The shim export lists are generated
  from the installed packages at console build time, never hand-maintained
  ([slots-and-api.md § shims](slots-and-api.md#sharing-one-react-the-boot-contract)).

## Testing a worker's UI assets

- **Unit-test the content function** like any function (path dispatch, unknown
  path errors).
- **e2e smoke** (workers repo `e2e/` harness): boot engine + console + your
  worker; assert `console::ui-manifest` lists your paths with non-empty
  hashes; `GET :3113/ui/<path>` returns your bytes; re-register with changed
  content and assert the manifest hash moved. That exercises the whole
  pipeline without a browser.
- Rendering correctness stays a browser concern — Storybook in the worker's
  own repo against the published `@iii/console` types is the recommended
  harness (the console already uses Storybook 10 for its own components).
