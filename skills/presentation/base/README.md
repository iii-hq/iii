# tech-spec presentations — one site

One Vite project that builds every tech spec into **__SITE_HOST__/roadmap/** — a
roadmap at `/` (a one-column timeline, newest spec first, grouped by month),
one page per spec at `/<slug>/` (an interactive deck when one exists, a
rendered markdown viewer when only the spec does), the raw `.md` linkable
beside it, and `index.json` feeding the __SITE_HOST__ landing timeline.

## the two trees

```
tech-specs/<slug>/*.md            the spec — MARKDOWN ONLY, frontmatter in README.md
website/roadmap/<slug>/     the deck's content layer (optional, pairs by dirname)
website/roadmap/src/        the shared design system + component library
```

The slug is the directory basename, used identically in three places: the spec
dir, the deck dir, and the URL `/roadmap/<slug>/`. Never prettify it; never
rename after publishing.

## the conflict contract (read this first)

```
SHARED / HOT — never edited when adding a spec or deck:
  src/**  build.mjs  vite.config.ts  tsconfig*  package.json  index.html  _viewer/**
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
`index.html` + `src/{App,sections,pages,content,spec-docs}` — no package.json,
no config), reuses the shared library via `@lib`, updates the spec's
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
`node build.mjs` warns on unregistered files, `--strict-registry` fails.

Never fork a shared component into a deck to tweak it — extend it with additive
props, or build a genuinely different deck-local one. Changing an EXISTING
shared component re-renders every deck: that is a design-system PR (full
`node build.mjs` verify), not a spec PR.

## d. local dev

```
pnpm install                          # repo root (workspace)
pnpm dev                              # ONE server: gallery at /, decks at /<slug>/
pnpm type-check                       # strict, shared lib + every deck
node build.mjs --only=<slug>          # fast: gallery + one spec
pnpm build && pnpm preview            # the full site at :4173
```

## e. ship

Open a PR — expected surface is the two per-spec paths only (reviewers should
bounce anything else). On merge to main, `your static-site CI`
builds this project and syncs `dist/` to S3 under `/roadmap/` + invalidates
CloudFront. No Vercel, no manual deploy, no per-deck pipeline.

URLs: `__SITE_HOST__/roadmap/` (gallery) · `__SITE_HOST__/roadmap/<slug>/` (deck or
viewer) · `…/<slug>/#/spec` (a deck's reading mode) · `…/<slug>/<file>.md`
(raw markdown) · `__SITE_HOST__/roadmap/index.json` (machine-readable list).

## f. troubleshooting

- card 404s → deck dirname ≠ spec dirname (the pairing contract)
- frontmatter warnings → schema table in (a); the build prints the exact rule
- `unregistered component` → procedure (c)
- orphan deck error → the spec dir moved/renamed underneath its deck
