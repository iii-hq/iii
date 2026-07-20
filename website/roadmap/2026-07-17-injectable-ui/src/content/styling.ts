/* styling — authored vs shipped css, the preset, the honest residue. */

export const CSS_AUTHORED = `// ui/page.tsx — vanilla tailwind, no prefixes
<div className="flex flex-col gap-2 p-4">
  <span className="text-sm">…</span>
</div>

/* ui/styles.css — the entire tailwind entry */
@import "@iii/console/tailwind.css";
@source "./";`

export const CSS_SHIPPED = `/* dist/ui/styles.css — every rule under the scope */
[data-iii-ui="state"] .flex { display: flex }
[data-iii-ui="state"] .gap-2 { gap: … }

/* :root / html / body rewritten to the scope root —
   custom properties inherit down the subtree only */
[data-iii-ui="state"] { --spacing: 0.25rem }`

export const PRESET_CELLS = [
  {
    title: 'utilities only',
    body: 'never tailwind’s preflight — a later-loaded reset would re-style the console document, and version skew between tailwind copies becomes console-wide visual drift.',
  },
  {
    title: 'dark: bound to data-theme',
    body: 'a @custom-variant ties dark: to html[data-theme="dark"], the console’s actual toggle — not the v4 default prefers-color-scheme.',
  },
  {
    title: '@theme inline token map',
    body: 'bg-accent, text-ink-faint, bg-panel compile to var(--color-*) references. nothing re-emitted at :root; injected ui themes with the console automatically.',
  },
  {
    title: 'generated with the shims',
    body: 'the preset ships in @iii/console, generated at console build time alongside the /vendor shims. a ci assertion keeps the token map ⊇ the console’s @theme.',
  },
] as const

export const STYLING_RESIDUE = [
  {
    name: 'no preflight inside slots',
    type: 'by design',
    desc: 'the console document’s base styles apply to injected markup — it is why injected ui looks native. an optional scoped reset ships beside the preset.',
  },
  {
    name: 'worker-owned portals',
    type: 'caveat',
    desc: 'host.components.Dialog is pre-stamped by the host; dom a worker portals to document.body itself must carry data-iii-ui on its root.',
  },
  {
    name: 'not a security boundary',
    type: 'warn-only lint',
    desc: 'scoping is compile-side hygiene. the console scans style assets for escaping selectors and attaches warnings to the manifest — never a rejection.',
  },
] as const
