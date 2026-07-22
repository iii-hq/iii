# Slots & the common API

What an injected script can touch: the shared-React boot contract, the
`@iii/console` module, the `setup(host)` lifecycle, the four slot kinds with
exact props, and the console refactors that make the slots exist.

## Sharing one React (the boot contract)

Two React instances break hooks and context, so injected scripts **must**
resolve `react` to the console's own instance. Today nothing enables that:
react/react-dom 19.2.6 are bundled into content-hashed Vite chunks, nothing is
on `window`, there is no import map, and the app has no runtime code loading
at all (sole dynamic import is a cycle-breaker,
`workers/console/web/src/pages/Workers/lib/merge-workers.ts:184`). The contract
is three small, purely additive pieces:

**1. The global.** First thing in `main.tsx`, before anything else can run:

```ts
import * as React from 'react'
import * as ReactDOM from 'react-dom'
import * as ReactDOMClient from 'react-dom/client'
import * as JsxRuntime from 'react/jsx-runtime'

window.__III_CONSOLE__ = Object.freeze({
  React, ReactDOM, ReactDOMClient, JsxRuntime,
  api: consoleApi,          // the @iii/console surface, below
})
```

Ordering is guaranteed structurally: injected modules are only ever imported by
the loader, which the SPA starts after this assignment.

**2. The import map.** One **static** `<script type="importmap">` in
`web/index.html`, placed before the Vite module script *and any
`<link rel="modulepreload">`* in document order (today's build emits none,
but Vite adds them for chunked builds). Under the classic single-import-map
semantics this design targets, a map parsed after *any* module loading has
been triggered is rejected wholesale — so the map must be static in the served
HTML and never injected dynamically. ("Present before the first
bare-specifier resolution" is the newer multiple/dynamic-map relaxation this
design deliberately does not depend on — its support is still uneven; model
knowledge, 2026.) The console's own bundle contains no bare imports at
runtime, so the map affects only injected modules. Static single import maps
are long-universal in evergreen browsers.

```html
<script type="importmap">
{ "imports": {
    "react":             "./vendor/react.js",
    "react-dom":         "./vendor/react-dom.js",
    "react-dom/client":  "./vendor/react-dom-client.js",
    "react/jsx-runtime": "./vendor/jsx-runtime.js",
    "@iii/console":      "./vendor/console-api.js"
} }
</script>
```

(Relative URLs on purpose — the build uses `base: './'` for arbitrary subpath
mounting, `workers/console/web/vite.config.ts:8-42`.)

**3. The shims.** Static ESM files under `web/public/vendor/` (Vite copies
`public/` into `dist/`, rust-embed ships them, the new `/vendor/*path` route
serves them — [hot-reload.md § Serving](hot-reload.md#serving-two-new-axum-routes)).
Each re-exports from the global; named exports must be enumerated statically
(ESM requires it). Abridged example:

```js
// web/public/vendor/react.js — GENERATED, abridged here
const R = window.__III_CONSOLE__.React
export default R
export const {
  Children, Component, Fragment, PureComponent, Profiler, StrictMode, Suspense,
  cloneElement, createContext, createElement, createRef, forwardRef, isValidElement,
  lazy, memo, startTransition, use, useCallback, useContext, useEffect, useMemo,
  useReducer, useRef, useState, useSyncExternalStore, useTransition, version,
  /* … full production export set … */
} = R
```

Hand-pinning that list would be a hazard: ESM named imports are checked at
resolution time, so one omitted export (`cache`, `useOptimistic`, …) inside
any dependency a worker bundled in is a whole-script `SyntaxError` at
`import()` time, not a graceful degradation. The four shims are therefore
**generated at console build time** from the installed packages' production
export sets (`Object.keys` per module, `__`-prefixed internals skipped), with
a CI assertion that each shim's exports ⊇ the production exports of `react`,
`react-dom`, `react-dom/client`, and `react/jsx-runtime`.

Result: a worker author writes ordinary `import { useState } from 'react'` and
JSX, bundles with those five specifiers marked external
([authoring.md](authoring.md)), and gets the console's living React at runtime.

## `@iii/console` — the module surface

The import exists for *shared singletons and types*; per-script registration
goes through `setup(host)` (next section) so the loader can dispose it.

```ts
// what ./vendor/console-api.js re-exports from window.__III_CONSOLE__.api
export interface ConsoleApi {
  /** The SPA's live engine client — the existing singleton's surface
   *  (workers/console/web/src/lib/iii-client.ts:32-65) MINUS dispose():
   *  the client is shared by the whole tab (traces, chat, every extension),
   *  so a script must never be able to tear it down. Because TS types erase
   *  and Object.freeze is shallow, api.iii is a narrowed wrapper object that
   *  simply does not carry dispose — not the raw IiiClient. */
  iii: {
    browserId: string                                        // `console-<uuid>`, per tab
    trigger<T = unknown>(functionId: string, payload?: Record<string, unknown>): Promise<T>
    on<P = unknown>(functionId: string, handler: (payload: P) => void | Promise<void>,
                    options?: RegisterFunctionOptions): () => void
    registerTrigger(input: RegisterTriggerInput): () => void
    addConnectionStateListener(handler: (state: IIIConnectionState) => void): () => void
  }
  /** Curated component library — re-exports from src/components/ui/ :
   *  Badge, Button, Dialog, DropdownMenu, EmptyState, ErrorBoundary, Input,
   *  Select, Skeleton, StatusDot, StatusPanel, Tabs, Tooltip
   *  plus JsonHighlight / CodeHighlight (lib/syntax.tsx) and Markdown (lib/markdown.tsx). */
  components: Record<string, React.ComponentType<any>>
  /** 'light' | 'dark', reactive (backed by html[data-theme] +
   *  the existing MutationObserver hook, web/src/hooks/use-theme.ts). */
  useTheme(): 'light' | 'dark'
  /** Design-token names, for documentation/tooling; styling just uses
   *  var(--color-*) (web/src/index.css:12-74). */
  tokens: readonly string[]
}
```

Notes:

- `iii.on()` already namespaces handlers per tab (`<functionId>::<browserId>`)
  and defaults `metadata.internal = true`
  (`workers/console/web/src/lib/iii-client.ts:162-180`) — injected scripts get
  multi-tab-safe, catalog-invisible live subscriptions with zero extra rules.
- The curated `components` list is the API contract; everything else in
  `src/components/ui/` stays console-private until promoted deliberately.
- Styling: the console's own Tailwind utility classes are **not** part of the
  contract (class names are build-artifacts of the console bundle). Injected
  UI styles with the CSS custom properties, inline styles, or its own `style`
  asset — typically Tailwind compiled through `@iii-dev/console-build`
  against the `@iii/console/tailwind.css` preset, scoped to the script's
  wrapper ([§ The scope wrapper](#the-scope-wrapper);
  [authoring.md § Styling & Tailwind](authoring.md#styling--tailwind)).
  Components from `api.components` arrive pre-styled.

## The script module contract: `setup(host)`

```ts
// the ONLY required export of a script asset
export default function setup(host: Host): void | (() => void) | Promise<void | (() => void)>

interface Host {
  /** Everything on ConsoleApi, flattened in for convenience. */
  iii: ConsoleApi['iii']
  components: ConsoleApi['components']
  useTheme: ConsoleApi['useTheme']
  /** Identity */
  path: string                       // e.g. "state/page.js"
  /** The four slot registrars — every call returns an unregister fn AND is
   *  auto-tracked: the loader runs all of them on dispose (hot reload,
   *  delete, worker disconnect). */
  slots: {
    register(slotId: 'composer.actions', component: React.ComponentType<ComposerActionProps>): () => void
  }
  pages: {
    register(page: PageRegistration): () => void
  }
  functionCalls: {
    register(renderer: FunctionCallRenderer): () => void
  }
  configForms: {
    register(configurationId: string, component: React.ComponentType<ConfigFormProps>): () => void
  }
}
```

Rules:

- Registration through `host` **only**. That is what makes hot reload sound:
  the loader disposes exactly what the script registered, then re-runs
  `setup()` on the new module ([hot-reload.md § loader](hot-reload.md#the-browser-loader)).
- `setup()` may return a teardown for anything the host can't track (timers,
  external listeners); it runs first on dispose (LIFO overall).
- Every injected component render is fenced by the existing `ErrorBoundary`
  (`fallback(error)` render prop,
  `workers/console/web/src/components/ui/ErrorBoundary.tsx:8-41`) showing a
  small chip with the script path — a crashing extension degrades to a chip,
  never a white screen.
- Multiple scripts may register into the same slot; entries render in
  registration order, keyed by `path` + registration index.

Slot registries are plain versioned maps read with `useSyncExternalStore`, so
host components re-render exactly when a script (re)registers — this is the
runtime realization of the registry sketched (and explicitly not implemented)
in `workers/console/docs/custom-function-call-message.md` §12.

### The scope wrapper

Every injected render mounts inside a host-owned element:

```html
<div data-iii-ui="state" style="display: contents">…</div>
```

- **Value = the first segment of the script's path** (`state/page.js` →
  `state`) — the same prefix that already serves as the asset's human
  attribution, so one worker's page, composer control, and function-call
  renderer share one scope, and its stylesheet (`state/theme.css`) targets
  all of them.
- **`display: contents`** removes the wrapper from layout — a slot in a flex
  row behaves as if the injected component were a direct child; the element
  exists only as a CSS ancestor.
- **All four registrars apply it**: slot outlets and pages wrap the
  component; `functionCalls` renderers get their non-null `tryRender*`
  returns wrapped by `FunctionCallCard`; config forms are wrapped inside
  `WorkerEditor`.
- **`host.components.Dialog` is stamped per script**: Radix dialogs portal
  their content to `document.body` — outside every wrapper — so the
  per-script `components` object re-exports `Dialog` with the scope
  attribute applied to the portaled content element. Without this, a
  worker's scoped styles would go dead exactly and only inside dialogs. DOM
  a script portals *itself* must carry the attribute on its own root
  ([authoring.md § Styling & Tailwind](authoring.md#styling--tailwind)).

The wrapper is what makes worker stylesheets composable: worker CSS compiles
with every selector under its own attribute
([authoring.md § Styling & Tailwind](authoring.md#styling--tailwind)), so N
workers' Tailwind builds coexist with each other and with the console's
fully-layered CSS — no class prefixes, no load-order sensitivity, no `:root`
leakage. It is a styling contract, not a sandbox (README non-goals): nothing
*forces* a hand-built sheet through the scoper; the console's style lint
warns on escapes
([injection-protocol.md § Style lint](injection-protocol.md#style-lint-warn-only)).

## The four slot kinds

One scope rule spans all four, stated once (it is a deliberate v1 decision,
not four coincidences — see README non-goals): **overrides are render-level.**
Each slot below names things its components *cannot* touch — the submit
payload, approval actions, the save/reset pipeline, error mapping. Those walls
are the same wall: injected UI replaces what is drawn and acts through
`host.iii`, never by intercepting a host pipeline.

### 1. `composer.actions` — extend the chat composer

Mount point: the composer footer's left flex-wrap picker group — today a
hardcoded row of `ModePicker`, optional `DirectoryPicker`/`BankPicker`/
`PermissionModePicker`, `ModelPicker`, each gated by a boolean prop wired from
worker presence (`workers/console/web/src/components/chat/Composer.tsx:332-375`).
A `<SlotOutlet id="composer.actions" …/>` is appended to that group.

```ts
interface ComposerActionProps {
  /** Append text to the draft as its own paragraph, focus the editor, and
   *  place the caret after it (whitespace-only drafts are replaced) — the
   *  same insertion path the Browser page already uses cross-surface
   *  (insertIntoComposer, workers/console/web/src/lib/composer-insert.ts:1-31,
   *  consumed by LexicalShell's ExternalInsertPlugin). NOT caret-position
   *  insertion; caret-aware insert would be new Lexical work. */
  appendText(text: string): void
  /** True while a submission is in flight (mirrors the send/stop button state). */
  busy: boolean
}
```

v1 composer extensions are **additive controls**, with two channels for doing
something: appending text to the draft (`appendText`), and acting over the bus
(`host.iii` — e.g. a dropdown that persists its selection to its own worker,
which consults it server-side). What v1 deliberately does **not** offer is
influence over submission itself. `ComposerSubmitPayload`
(`{ text, attachments }`, `Composer.tsx:29-32`) has no extension field — and
note the first-party pickers' power doesn't come from the payload either, but
from mutating *lifted conversation state* (mode, model, memory bank, working
dir) threaded through ChatView's submit path, which is unreachable from the
injected surface. A worker cannot ship a picker-equivalent in v1; that
requires an extension channel on the submit path (an `extensions` bag on the
payload, or a scoped conversation-state API) — named v2 work, a chat-pipeline
change rather than a UI slot (see README non-goals).

### 2. `functionCalls` — override how a function call renders

The registry entry is the §12 sketch, extended with a `tryRenderRunning`
member (a deliberate, named divergence from the doc's sketch — all 13 families
already export it, today only as an unconsumed symmetry alias of `tryRender`;
the registry makes it callable):

```ts
interface FunctionCallRenderer {
  id: string                                     // e.g. "state/page.js#renderer"
  isMatch(functionId: string): boolean
  tryRender(message: FunctionCallMessage): React.ReactNode | null
  tryRenderRunning?(message: FunctionCallMessage): React.ReactNode | null
  tryRenderPreview?(message: FunctionCallMessage): React.ReactNode | null
  FunctionIdLabel?(props: { functionId: string }): React.ReactNode
  primaryTabLabel?: string
}
```

`FunctionCallMessage` is the existing contract, unchanged
(`workers/console/web/src/types/chat.ts:105-134` — `role: 'function-call'`,
`functionId`, `input`, `output?`, `durationMs?`, `running?`, …).

Dispatch order: **injected renderers first** (registration order), then the 13
first-party families, then the JSON fallback — first non-null wins, `null`
always means "fall through". "Override a function component" is therefore
literal: match the built-in family's ids and return non-null. Host semantics
the sketch's doc already fixes stay owned by the host: errors parse before
success, the approval bar is host-rendered, never renderer-rendered. Because
`FunctionCallCard` is props-only and location-agnostic (chat + traces span tab,
`workers/console/web/src/components/function-call/FunctionCallCard.tsx:46-57`),
injected renderers apply in both places with no extra work.

### 3. `pages` — contribute whole pages

```ts
interface PageRegistration {
  id: string                        // kebab-case, unique per tab; convention "<worker>-<name>"
  title: string                     // nav label
  render: React.ComponentType       // the page body (right pane)
}
```

- Route: `#/ext/<id>` — a new prefix in the hash router, deliberately outside
  the first-party `View` union
  (`workers/console/web/src/hooks/use-hash-route.ts:8-14`) so injected pages
  can never collide with or shadow `#/traces`, `#/workers`, ….
- Nav: appended to the options `buildViewOptions` produces
  (`workers/console/web/src/lib/nav-options.ts:9-28`) — the runtime analogue of
  the existing worker-presence gating, except presence is now *the script being
  loaded* (which already tracks worker connectedness via trigger GC).
- Unregister/dispose while the page is active: the router falls back to the
  default view (`#/traces`), mirroring today's default-view behavior.
- Duplicate `id`: last registration wins, `console.warn` names both paths.
- **Windows**: there is no dedicated window slot — a page (or any slot
  component) opens modal/floating surfaces by rendering
  `host.components.Dialog` with state it controls. Pages + `Dialog` are the
  "new windows" story (see README non-goals).

### 4. `configForms` — override a configuration form by configuration id

Configuration entries are keyed by id end-to-end
(`configuration::list/schema/get/set`,
`workers/console/web/src/pages/Configuration/tabs/WorkersTab/api.ts:80-120`),
and rendered today by the structural `SchemaForm`. The override replaces the
**form region** inside `WorkerEditor` — the `SchemaForm` render when a schema
exists, or the "no editable configuration" `EmptyState` when none does —
while dialog chrome, dirty-state, save/reset, and error mapping stay
host-owned, so an override cannot break persistence.

```ts
interface ConfigFormProps {
  id: string                        // configuration id, e.g. "state"
  /** Deliberately WIDER than SchemaFormProps.schema (non-nullable JsonSchema,
   *  .../WorkersTab/schema-form/SchemaForm.tsx:21-31): null means the worker
   *  registered a configuration value but no JSON schema
   *  (ConfigurationSchemaView.schema, .../WorkersTab/api.ts:44-51) — the one
   *  branch where SchemaForm cannot render at all, and arguably the
   *  highest-value case for a custom form. */
  schema: JsonSchema | null
  value: JsonValue
  onChange(next: JsonValue): void
  errors?: ReadonlyMap<string, string>   // JSON-pointer → message
  /** Deep-link focus request: the decoded fieldPath segments from
   *  #/workers/configuration/<id>/<fieldPath> — string[] exactly as the route
   *  carries them (use-hash-route.ts:21-69); never a joined string (keys may
   *  contain '/'). Absent when the dialog wasn't opened via a field link. */
  focusField?: readonly string[]
}
```

Lookup: exact configuration-id match; no override registered ⇒ today's
behavior, unchanged (`SchemaForm`, or the `EmptyState` in the null-schema
branch). Field deep-links keep resolving to the dialog, but the host's own
scroll+focus effect targets DOM ids only the schema-form field components
render — so it silently no-ops inside a custom form, and honoring
`focusField` is the override's job, by design.

## Prerequisite console refactors

Runtime slots need seams the compile-time console doesn't have yet. All five
are mechanical, first-party-behavior-preserving, and land before or with the
loader:

1. **FunctionCallCard registry.** Replace the three hand-written `??`/`if`
   chains — the `FunctionIdLabel` if-chain (`FunctionCallCard.tsx:206-247`),
   the `tryRenderPreview` chain and the `tryRender` chain (`:279-307`) — with
   iteration over an ordered registry array; the 13 families become 13 registry
   entries with unchanged order (they already export the right shape:
   `is<Family>Function`, `tryRender`, `tryRenderRunning` — currently an
   unconsumed alias, so there is no running chain to replace —
   `tryRenderPreview`, label component). Injected entries prepend. —
   *Recommended in the same change; keeping the chains and consulting the
   registry first is a smaller but uglier fallback.*
2. **Dynamic pages.** Teach `use-hash-route.ts` the `#/ext/<id>` prefix and
   `App.tsx`'s ternary chain (`App.tsx:93-105`) a lookup into the page
   registry; extend nav options with registered pages.
3. **Composer outlet.** Add the `SlotOutlet` to the footer picker group and
   thread `appendText`/`busy` (`Composer.tsx:332-375`).
4. **WorkerEditor override lookup.** One registry consultation covering both
   form-region branches (the `SchemaForm` render and the null-schema
   `EmptyState`).
5. **Scope wrapper + stamped Dialog.** Mount every injected render inside its
   `data-iii-ui` element (slot outlet, page body, non-null injected
   `tryRender*` results in `FunctionCallCard`, the config-form region), and
   give each script's `host.components` a `Dialog` re-export whose portaled
   content carries the script's scope attribute
   ([§ The scope wrapper](#the-scope-wrapper)).

Plus the worker-side seams from the other documents: router-state widening +
`/ui` + `/vendor` routes ([hot-reload.md](hot-reload.md)), trigger-type
handler ([injection-protocol.md](injection-protocol.md)), and the
`main.tsx` boot contract above.
