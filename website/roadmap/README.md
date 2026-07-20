# tech-spec roadmap

The shared library and content layers behind **iii.dev/roadmap/** — a roadmap
at `/roadmap/` (a one-column timeline, newest spec first, grouped by month),
one page per spec at `/roadmap/<slug>/` (an interactive deck when one exists,
a rendered markdown viewer when only the spec does), the raw `.md` linkable
beside it, and `/roadmap/index.json` feeding the iii.dev landing timeline.

There is no separate build here: the decks are **routes of the iii-website
Astro app**. The pages at [`../src/pages/roadmap/`](../src/pages/roadmap/)
discover specs through [`scripts/manifest.mjs`](./scripts/manifest.mjs) and
mount each deck's `src/App.tsx` as a code-split React island
([`src/DeckHost.tsx`](./src/DeckHost.tsx)). This directory holds the shared
design system (`src/`, imported as `@lib`), the md-only viewer (`_viewer/`),
and one content-layer dir per deck. No package.json, no vite config, no
index.html — deps live in [`../package.json`](../package.json).

## the two trees

```
tech-specs/<slug>/*.md         the spec — MARKDOWN ONLY, frontmatter in README.md
website/roadmap/<slug>/        the deck's content layer (optional, pairs by dirname)
website/roadmap/src/           the shared design system + component library
```

The slug is the directory basename, used identically in three places: the spec
dir, the deck dir, and the URL `/roadmap/<slug>/`. Never prettify it; never
rename after publishing.

## the conflict contract (read this first)

```
SHARED / HOT — never edited when adding a spec or deck:
  src/**  scripts/manifest.mjs  _viewer/**
  website/src/pages/roadmap/**  website/scripts/validate-roadmap.ts
  (COMPONENTS.md: append-only, component promotions only)

PER-SPEC — the only things a spec/deck PR touches:
  tech-specs/<slug>/**        website/roadmap/<slug>/**
```

Corollary: two spec PRs never conflict. A component promotion is the deliberate
exception (procedure c).

## a. add a tech spec

Via `/tech-spec`, or by hand:

1. `mkdir tech-specs/YYYY-MM-DD-<slug>/` — the dirname IS the url slug; the
   day prefix is what orders and labels the roadmap timeline.
2. Write `README.md` (the index/overview) + one `.md` per domain topic. Markdown only.
3. Top of `README.md`, the frontmatter block:

```yaml
---
title: the developer experience overhaul        # fallback: first H1
tagline: one file, one command, zero zombies.   # fallback: first paragraph
date: 2026-06-21                                # YYYY-MM-DD; fallback: dirname
                                                # prefix. day precision drives
                                                # the roadmap order + labels
tags: [dx, cli]                                 # ≤ 4
status: live                                    # or draft (muted card, kept
                                                # out of index.json + sitemap)
featured: false                                 # pin in the landing feed
                                                # (index.json); the roadmap
                                                # itself stays chronological
---
```

`slug` is never a frontmatter field — the build hard-errors if present.

4. Merge. The site now serves `/roadmap/<slug>/` as a rendered spec viewer,
   the gallery gets a card, and the landing timeline picks it up. A deck can
   come later — or never.

## b. add a presentation to a spec

Run `/presentation tech-specs/<slug>` — it proposes a narrative outline (gate 1:
your approval), scaffolds `website/roadmap/<slug>/` (content layer only:
`src/{App,sections,pages,content,spec-docs}` — no index.html, no main.tsx,
no package.json, no config; the `[slug]/index.astro` route provides the
document shell), reuses the shared library via `@lib`, updates the spec's
frontmatter, and verifies (gate 2: typecheck + build + browser pass).

It may touch ONLY `website/roadmap/<slug>/**`, the spec README's
frontmatter block, and (rarely) an additive component promotion per (c).

The one fragile cross-tree coupling: `src/spec-docs.ts` bundles the spec md via
`import.meta.glob('../../../../tech-specs/<slug>/*.md', …)` — the depth encodes
the two-tree layout.

## c. add or promote a shared component

Default is **deck-local**: `website/roadmap/<slug>/src/diagrams/<Name>.tsx`.
Promote into `src/components/` only when all three hold: props-driven with zero
spec data inside; maps to a recurring spec shape (a lifecycle, a tree, a
timeline, a fan-out…); passes the standards checklist (design tokens only,
reduced-motion gate, keyboard operable, aria-label, container queries,
overflow-x-auto + min-w, no shadows/gradients, documented props). A promotion
MUST add its [COMPONENTS.md](./COMPONENTS.md) entry in the same change —
`pnpm build` (via `scripts/validate-roadmap.ts`) warns on unregistered files,
`--strict` fails.

Never fork a shared component into a deck to tweak it — extend it with additive
props, or build a genuinely different deck-local one. Changing an EXISTING
shared component re-renders every deck: that is a design-system PR (full
build verify), not a spec PR.

## d. local dev

```
pnpm install        # repo root (workspace)
pnpm dev            # from website/: THE dev server — whole site at :4321,
                    # this tree served at /roadmap/ with HMR
pnpm type-check     # from website/: strict TS over shared lib + every deck
pnpm build          # from website/: contract checks + full site → ../dist/
pnpm preview        # from website/: the built site at :4321
```

(From the repo root, prefix with `pnpm --filter iii-website …`.)

## e. ship

Open a PR — expected surface is the two per-spec paths only (reviewers should
bounce anything else). On merge to main, `.github/workflows/deploy-website.yml`
runs the single website build (deck pages emit into `dist/roadmap/`), syncs
`website/dist/` to S3, and invalidates CloudFront. No Vercel, no manual
deploy, no per-deck pipeline.

URLs: `iii.dev/roadmap/` (gallery) · `iii.dev/roadmap/<slug>/` (deck or
viewer) · `…/<slug>/#/spec` (a deck's reading mode) · `…/<slug>/<file>.md`
(raw markdown) · `iii.dev/roadmap/index.json` (machine-readable list).

## f. troubleshooting

- card 404s → deck dirname ≠ spec dirname (the pairing contract)
- frontmatter warnings → schema table in (a); `pnpm build` prints the exact rule
- `unregistered component` → procedure (c)
- orphan deck error (`validate-roadmap`) → the spec dir moved/renamed
  underneath its deck
