# /presentation

Turn a tech-spec directory into an interactive, marketing-grade web
presentation — the kind at [iii.dev/roadmap/](https://iii.dev/roadmap/).

The deck is built to: help engineers **understand** the design (the
architecture is a navigable map, not prose), be **interactive** (steppable
diagrams, a selectable system map, live toggles), read like **marketing**
(it argues the *why*), and be **build-in-public ready** (each deck builds to a
portable static `dist/<slug>/`).

## Use it

From a Claude Code session in this workspace:

```
/presentation tech-specs/2026-06-devexp
```

It reads the spec, reads the repo's component registry, proposes a
slide-by-slide narrative outline for your approval, scaffolds the deck's
**content layer** at `<base>/<slug>/` (in iii: `website/roadmap/<slug>/`),
generates the content against the shared `@lib` component library, writes the
spec's registration frontmatter, and verifies with a typecheck, a build, and a
browser pass.

## Hosting — one Vite project per repo, no per-deck deploys

Specs are markdown-only at `tech-specs/<slug>/`; decks are content layers
inside the repo's single presentations site (the "base"). The base owns the
one `package.json`, the shared component library + design tokens, the gallery,
a markdown viewer for specs without decks, and `build.mjs`. In iii, merging to
main deploys everything to `iii.dev/roadmap/` via the website workflow —
no Vercel, no manual step. See `reference/hosting.md`.

## What's in here

- `SKILL.md` — the operating instructions (the brain).
- `reference/` — the design system, the interactive archetype library, the
  component standards + registry format, the narrative framework, the quality
  bar, and the hosting/layout rules. The skill loads these first.
- `template/` — one deck's content-layer starter (~13 files: App, deck.ts,
  spec-docs glob, three worked-example sections, a deep-dive page). No
  package.json, no config — everything visual imports from `@lib`.
- `base/` — the full per-repo presentations site scaffold: shared `src/`
  (primitives, diagram archetypes, hooks, markdown machinery, gallery,
  viewer), `build.mjs`, `vite.config.ts`, tsconfigs, the `COMPONENTS.md`
  registry seed, and the SOP README seed. Copied once per repo, identity
  tokens filled, then only ever extended by component promotions.

## Extending the design

The live visual system for iii is `website/roadmap/src/` (tokens in
`src/index.css`, components under `src/components/`); every promoted component
gets a `COMPONENTS.md` entry per `reference/component-standards.md`.
**Maintenance note:** `base/` is a snapshot of that live `src/` — re-seed it
from `website/roadmap/` periodically (copy `src/`, `scripts/manifest.mjs`,
`vite.config.ts`, tsconfigs, `_viewer/`, `COMPONENTS.md`; take `build.mjs`
but restore the standalone `DIST = join(ROOT, 'dist')` — iii's copy emits
into `website/dist/roadmap/`; keep base's own `package.json`, since iii folds
those deps into the `iii-website` package; re-tokenize the `package.json`
name, `src/gallery/site.ts`, `index.html`, `README.md`) so fresh repos start
with the newest archetypes. The template imports only the
core `@lib` surface (Section, TopNav, Footer, Sheet, SpecSheet, PageShell,
hooks, SpecPage), which every base version guarantees.
