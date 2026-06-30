# /presentation

Turn a tech-spec directory into an interactive, marketing-grade web
presentation — the kind in `tech-specs/2026-06-devexp/presentation/` and
`tech-specs/2026-06-agentic/presentation/`.

The deck is built to: help engineers **understand** the design (the
architecture is a navigable map, not prose), be **interactive** (steppable
diagrams, a selectable system map, live toggles), read like **marketing**
(it argues the *why*), and be **build-in-public ready** (a portable static
site).

## Use it

From a Claude Code session in this workspace:

```
/presentation iii/tech-specs/2026-06-devexp
```

It reads the spec, proposes a slide-by-slide narrative outline for your
approval, scaffolds the project from `template/`, generates the content,
registers the deck in the repo's base site, and verifies it with a typecheck, a
build, and a browser pass. Output lands in `<spec-dir>/presentation/` by default.

## Hosting — one Vercel project per repo

Decks are not deployed one by one. Each repo gets a **base project at
`<repo>/tech-specs/`** that hosts all of its presentations under a single Vercel
project: a gallery index at `/` and each deck at `/<slug>/`. The skill creates
it from the bundled `base/` scaffold the first time a repo needs it, then only
appends to its manifest. See `reference/hosting.md`.

## What's in here

- `SKILL.md` — the operating instructions (the brain).
- `reference/` — the design system, the interactive archetype library, the
  narrative framework, the quality bar, and the hosting/base-project rules. The
  skill loads these first.
- `template/` — a frozen, build-verified Vite + React + TS scaffold for one
  presentation. Each run copies it verbatim and generates only the content
  layer, so the design system never drifts. Run
  `pnpm install --ignore-workspace && pnpm dev` inside it to preview the deck.
- `base/` — a frozen scaffold for the per-repo host site: a gallery (`_gallery/`,
  reusing the same design system) plus `build.mjs`/`vercel.json` that assemble
  the gallery and every `<spec>/presentation/` into one deployable `dist/`. After
  a one-time identity setup (repo name + gallery title/description), each new deck
  only appends an entry to `_gallery/src/content/presentations.ts` and its card
  works out-of-the-box.

## Extending the design

The visual system lives in `template/src/index.css` and
`template/src/components/`. Edit those to evolve the look for every future
deck. The archetype library is documented in `reference/archetypes.md`.
