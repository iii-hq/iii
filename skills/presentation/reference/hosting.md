# Hosting — the base project (one Vercel project per repo)

Every repo that holds tech-specs gets **one base project at `<repo>/tech-specs/`
that hosts all of its presentations**. A deck is never deployed alone; it is
registered into the base project, and the base project deploys as a single
Vercel site. This file is the law for that layer — do not re-derive it.

## Layout

```
<repo>/tech-specs/                 the base project = the Vercel deploy unit
├── vercel.json                    deploy config (root directory = this folder)
├── build.mjs                      orchestrator (gallery + every presentation -> dist/)
├── package.json                   dev / build / preview scripts
├── _gallery/                      the index site (Vite + React, design system)
│   └── src/content/presentations.ts   the manifest — the one file to fill in
├── <spec-dir>/*.md                the spec's markdown (the deck's `#/spec` viewer
│                                    bundles these at build — another reason the
│                                    deck must live *inside* the spec dir)
├── <spec-dir>/presentation/       one standalone deck per spec (unchanged)
└── dist/                          build output Vercel serves (gitignored)
```

Built output served by Vercel:

```
/                 the gallery (index of every deck)
/<slug>/          one presentation per spec
```

The base project lives in the skill at `base/`. The skill copies it into a repo
the first time that repo needs it, then only ever edits the manifest.

## The slug contract — the one rule that keeps it coherent

**A deck's slug is its spec-directory name, used in three places that must match
exactly:**

1. the folder: `<repo>/tech-specs/<slug>/presentation/`
2. the URL: `/<slug>/` (the card's `href` is `<slug>/`)
3. the manifest entry's `slug` field

`build.mjs` discovers presentations by scanning for `<dir>/presentation/` and
copies each to `dist/<dir>/`. The gallery card links to `<slug>/`. If the
manifest slug ≠ the directory name, the card 404s. So: **slug = the spec
directory's basename. No prettifying, no date-stripping.**

## The manifest — the only file you fill in

`_gallery/src/content/presentations.ts` exports `GALLERY_META` (the site's
identity) and `PRESENTATIONS` (the deck list). It is pure data, no JSX.

`GALLERY_META` — set once when the base project is first created for a repo:

| field | what |
|---|---|
| `wordmarkLabel` | text by the wordmark, e.g. `"iii / tech-specs"` |
| `heroEyebrow` | small-caps eyebrow — ships pre-filled as `"tech-specs"`, no token; usually left as-is |
| `heroTitle` | the big hero line — what this collection is |
| `heroLead` | one–two sentences under the title |
| `attribution` | left footer line |
| `source` | right footer line, e.g. `"source of truth: <repo>/tech-specs"` |

`PRESENTATIONS[]` — append one per deck (the skill does this):

| field | what |
|---|---|
| `slug` | **= the spec directory name** (see the contract above) |
| `title` | deck title, lowercase |
| `tagline` | the single load-bearing claim, one lowercase line |
| `spec` | the spec path label, e.g. `"tech-specs/2026-06-devexp"` |
| `date` | label + sort key, e.g. `"2026-06"` |
| `tags?` | 0–4 short topic tags |
| `status?` | `"live"` (default) or `"draft"` (muted, flagged, still links) |
| `featured?` | pin to the top of the grid |

The gallery, cards, header, and footer are pre-built and read entirely from this
file. Adding a deck is: scaffold it, then add one entry here. Nothing else.

## What the skill touches vs. never touches

- **First time a repo needs it:** copy `base/` → `<repo>/tech-specs/`, then fill
  the `__PLACEHOLDER__` tokens — `package.json` `name` (`__REPO__`),
  `_gallery/index.html` (`__GALLERY_TITLE__`, `__GALLERY_DESCRIPTION__`), and
  `GALLERY_META` (the `__GALLERY_*__` fields). Delete the example
  `__EXAMPLE_SLUG__` entry in `PRESENTATIONS`.
- **Every run after that:** only append/update the deck's entry in
  `PRESENTATIONS`. Never edit `build.mjs`, `vercel.json`, the gallery
  components, or the design system — that layer is frozen, exactly like
  `template/`.

## Verify the base layer

From `<repo>/tech-specs/`:

```bash
node build.mjs --only=<slug>   # build the gallery + just this deck (fast)
pnpm preview                   # serve the full site at :4173, browse the card -> deck
```

`/browse` the gallery: the new card appears, its tags/date/status read right,
and clicking it lands on the deck at `/<slug>/` with no console errors.

## Deploy

One Vercel project per repo. Root Directory = `tech-specs`. Everything else
(`framework`, build/install/output) is set in `vercel.json`. Deploying is the
user's call — never run `vercel` yourself; hand them the steps in the base
project's `README.md`.
