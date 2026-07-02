# hosting — the two-tree layout, one site

Every repo hosts its spec presentations as **one Vite project** (the "base") —
no per-deck projects, no per-deck installs, no separate deploys. This file is
the law for that layer — do not re-derive it.

## The two trees

```
<repo>/tech-specs/<slug>/*.md          the spec — MARKDOWN ONLY
                                       (frontmatter in README.md = registration)
<base>/<slug>/                         the deck's content layer (optional)
<base>/src/                            the shared component library + tokens
```

In iii, `<base>` = `website/presentations/`. The pointer file
`tech-specs/README.md` names the base dir — that is how Phase 0 finds it in
any repo.

**The pairing contract.** `slug` = the spec directory's basename, used
identically in three places: `tech-specs/<slug>/` (the md), `<base>/<slug>/`
(the deck), and the URL `/tech-specs/<slug>/`. The build fails on an orphan
deck dir (no matching spec) and never needs a manifest — the folder is the
identity, so the old "manifest slug ≠ dirname → 404" bug class cannot exist.

## Registration = frontmatter (no central manifest)

The top of `tech-specs/<slug>/README.md`:

```yaml
---
title: the developer experience overhaul        # fallback: the first H1
tagline: one file, one command, zero zombies.   # fallback: first paragraph
date: 2026-06-21                                # YYYY-MM-DD; fallback: dirname
                                                # prefix. day precision drives
                                                # the roadmap order + labels
tags: [dx, cli]                                 # ≤ 4
status: live                                    # or draft — muted card, kept
                                                # out of index.json + sitemap
featured: false                                 # pin in the landing feed
                                                # (index.json); the roadmap
                                                # itself stays chronological
---
```

- `slug` is NEVER a field — the build hard-errors if present.
- Deck presence is derived (`<base>/<slug>/index.html` exists), never declared.
- A spec with no frontmatter still lists (the fallbacks apply); the build
  warns per derived field.
- A new spec touches only its own folder → two spec PRs can never conflict.

## What the build produces

`node build.mjs` (at `<base>`; `--only=<slug>` for a fast partial):

```
dist/index.html            the roadmap (a one-column timeline, newest spec
                           first, month-grouped; built from
                           virtual:spec-manifest)
dist/index.json            machine-readable spec list (feeds the iii.dev
                           landing timeline; drafts excluded)
dist/<slug>/index.html     the deck — or the generic md viewer (_viewer/,
                           built once + copied) when the spec has no deck
dist/<slug>/<file>.md      the raw spec markdown, directly linkable
dist/<slug>/spec.json      (viewer pages only) the file list the viewer fetches
```

Every deck builds with `base: './'` and its own hashed assets, so
`dist/<slug>/` stays individually portable — any CDN, any prefix, or straight
from disk.

## The spec-docs glob (the one fragile coupling)

Each deck's `src/spec-docs.ts` bundles its spec markdown at build time:

```ts
export const SPEC_DOCS = import.meta.glob('../../../../tech-specs/<slug>/*.md', {
  query: '?raw', import: 'default', eager: true,
}) as Record<string, string>
```

`import.meta.glob` resolves relative to the importing file, so the literal
encodes the depth from `<base>/<slug>/src/` to the spec tree. The skill
substitutes it at scaffold time (`__SPEC_MD_GLOB__`); if the base ever moves,
every deck's glob moves with it — `pnpm type-check`/`build` catch it.

## Dev, verify, ship

```
pnpm dev                          ONE server: gallery at /, every deck at /<slug>/
pnpm type-check                   strict, shared lib + gallery + viewer + all decks
node build.mjs --only=<slug>      gallery + one spec (fast)
node build.mjs --strict-registry  registry parity as a hard failure (CI)
pnpm build && pnpm preview        the full site at :4173
```

**Deploy (iii):** merging to main runs `.github/workflows/deploy-website.yml`,
which builds the base (`pnpm --filter iii-presentations build`) and syncs
`dist/` to S3 under the `/tech-specs/` prefix (immutable hashed assets;
must-revalidate html/json/md), then invalidates CloudFront. The CloudFront
viewer-request function rewrites `/tech-specs/…/` directory URLs to
`…/index.html` and 301s extensionless forms to the trailing-slash canonical —
same mechanism as `/blog/`. No Vercel, no manual deploy, no per-deck pipeline.

**Deploy (other repos):** `dist/` is fully static with relative asset paths —
publish it under any prefix with whatever CI the repo uses. The skill never
creates deploy config.

## Porting a legacy layout

A repo on the old model (standalone Vite project per deck +
`tech-specs/build.mjs` + `_gallery/`) ports one deck at a time:

1. Move the spec md to `tech-specs/<slug>/` (md only) and add the frontmatter
   block (values from the old `_gallery/src/content/presentations.ts` entry).
2. Move the deck's content layer (`index.html` → entry `./src/main.tsx`,
   `src/{App,sections,pages,content}`) to `<base>/<slug>/`.
3. Delete its duplicated machinery: package.json, lockfile, vite/tsconfigs,
   `src/{components,hooks,lib}`, `index.css`, markdown utils, SpecPage.
4. Rewrite imports (shared → `@lib/…`; deck-local → relative), thread
   `meta`/`nav`/`footer` props from `content/deck.ts`, add `src/spec-docs.ts`
   + `PAGES.spec`.
5. Truly spec-specific diagrams stay deck-local under `<slug>/src/diagrams/`;
   generic ones promote per `reference/component-standards.md`.
6. `pnpm type-check && node build.mjs --only=<slug>` + `/browse`; delete the
   old per-deck project and, when the last deck is ported, the old
   `_gallery/` + root glue.
