# iii.dev website

One [Next.js](https://nextjs.org) 16 app (App Router, static export) on Tailwind CSS v4 and
[shadcn/ui](https://ui.shadcn.com) over [Base UI](https://base-ui.com) (`base-nova` style) that builds **all of
iii.dev** into `dist/`: prerendered HTML, nothing rendered at request time. Deployed to S3 + CloudFront by
`.github/workflows/deploy-website.yml`; Vercel builds previews from the same `pnpm build`.

| URL | Source |
| --- | --- |
| `/` | `app/(site)/page.tsx` and `components/landing/` (copy in `components/landing/content.ts`) |
| `/manifesto` | `app/(site)/manifesto/` and `components/manifesto/` |
| `/privacy-policy` | `app/(site)/privacy-policy/` (copy in `privacy-data.ts`) |
| `/blog/`, `/blog/<slug>/`, `/blog/rss.xml`, `/blog/<slug>.md`, `/blog/index.md` | markdown posts in `content/blog/`; the `.md` twins are written by `scripts/generate-blog-md.ts` |
| `/roadmap/`, `/roadmap/<slug>/`, `/roadmap/<slug>/<file>.md`, `/roadmap/index.json` | tech specs read from `../tech-specs/` (markdown only, frontmatter in each `README.md`) |
| `/roadmap/<slug>/deck/` | the interactive presentation for a spec, from `roadmap/<slug>/src/App.tsx` ([roadmap/README.md](./roadmap/README.md)) |
| `/a` | the retired A/B test route; refreshes to `/` |
| `/sitemap.xml`, `/llms.txt`, `/AGENTS.md` | generated into `dist/` by `scripts/` after the Next build |
| `/robots.txt`, `/favicon.svg`, `/og-image.png`, `/posthog-consent.js`, `/blog/<slug>/*`, `/console-demo/*`, `/fonts/*` | `public/` (copied verbatim) |

`app/(site)` and `app/(deck)` are two root layouts: the site (fonts, analytics, cookie banner) and the decks (their
own Tailwind theme, `roadmap/src/index.css`). `app/not-found.tsx` carries its own document for the export's
`404.html`.

## Local development

```bash
pnpm install            # repo root (workspace)
pnpm dev                # http://localhost:3100 (regenerates the roadmap manifest first)
pnpm build              # the full site → dist/ (roadmap checks → manifest → next build →
                        #   dist shaping → blog .md → llms/AGENTS → sitemap)
pnpm preview            # serve dist/ at :4321
pnpm test               # script unit tests + CloudFront function tests (no build needed)
pnpm test:dist          # post-build contract for the whole dist/ tree (run after pnpm build)
pnpm type-check         # tsc over the site and the roadmap decks
pnpm lint               # biome
```

From the repo root: `pnpm --filter iii-website <script>`, or `pnpm dev:website`.

## How the export is shaped

Next writes every page as `<route>.html`. `scripts/finalize-dist.ts` then moves the pages under `blog/` and
`roadmap/` into `<route>/index.html`, because the CloudFront edge function
(`infra/terraform/website/cloudfront_functions/redirects.js`) serves those two prefixes as directory sites with
trailing-slash canonical URLs. Top-level pages stay `manifesto.html`, `privacy-policy.html`: the deploy syncs the
`/<page>` → `/<page>.html` map into a CloudFront KeyValueStore from `scripts/routes-kvs.ts`. Adding a page is a
content-only change. Do not change these shapes; they are the site's canonical URLs.

`scripts/generate-llms-agents.ts` reads the homepage copy from the built `dist/index.html`: the hero `<h1>` and its
paragraph, each section's head (eyebrow, `<h2>`, lede), the FAQ from the page's FAQPage JSON-LD, and the footer.
Keep those shapes when editing the landing; `pnpm test` checks the extractor against the built page.

## Writing a blog post

Add a markdown file at `content/blog/<slug>.md`; the filename is the URL (`/blog/<slug>/`). Frontmatter:

```yaml
---
title: 'The Harness Is the Backend'
description: 'One or two sentences; becomes the meta description and RSS blurb.'
pubDate: 2026-05-07
updatedDate: 2026-05-09      # optional
ogImage: ../../assets/blog/<slug>/banner.png   # optional; the file lives in public/blog/<slug>/
tags: [agents, architecture]
draft: false
---
```

Optional SEO fields: `seoTitle` replaces the `<title>` and share-card title when it should differ from the on-page
heading (the heading stays `title`); `faq` is a list of `{ q, a }` entries emitted as FAQPage JSON-LD next to the
BlogPosting entry, so keep the answers plain text and in sync with the section they summarise.

Images: put the files in `public/blog/<slug>/` and reference them as `../../assets/blog/<slug>/<file>` in the
markdown (the historical path, rewritten at render time and in the `.md` export).

Blocks a post can opt into, all plain HTML around ordinary markdown (blank lines on both sides) and styled in
`globals.css` under `.prose`: `<figure>` with an `<img>` and a `<figcaption>` for a captioned screenshot (the
`../../assets/...` path is rewritten like a markdown image); `<div class="facts">` around a bullet list of
`**Label:** value` items to show them as rows in a card; `<div class="verdict">` of `.verdict-card`s (eyebrow
`.verdict-pick`, `.verdict-tool`, `.verdict-reason`, `.verdict-cta` link; `data-highlight` marks the recommended
pick); and `<div class="compare">` around a GFM table to render it as a matrix, with the last column (iii, by
convention) emphasized and its header shown as the iii logo. `data-highlight` on a `.facts` div or a `<figure>` gives
it the same neutral-800 frame as the highlighted verdict card, whose name is also shown as the logo alone. Long-form
pages are black and white only: no accent colour, no glow, and code blocks render in greys. `####`
headings are label-style (small caps) and stay out of the contents list. See
`content/blog/opencode-vs-claude-code-vs-iii.md`.

## The benchmarks page

`/benchmarks` draws two data sets that CI publishes to the `gh-pages` branch:
- **Load benchmarks.** These come from the workers repo (`.github/workflows/loadtest.yml` and `sandbox-libkrun/crates/iii-loadtest`, which publish to this repo's `gh-pages`), and live in `loadtest/index.json` (every run, newest first) and `loadtest/runs/<id>.json`.
- **Criterion history.** This comes from `bench-release.yml` and lives in `dev/bench/data.js`.

`scripts/generate-benchmarks.ts` runs before `dev` and `build`. It fetches both and writes `content/benchmarks/data.json`, which is gitignored and is what the page imports. Each workflow dispatches a website deploy once it has published, so the page follows every run.

A part that cannot be fetched falls back to `content/benchmarks/snapshot.json`, which is committed, so a build never fails or ships an empty page. Two environment variables change where the data comes from:
- `BENCHMARKS_OFFLINE=1` skips the network.
- `BENCHMARKS_SOURCE` points at another copy of the branch, either a URL or a local checkout.

Every number in the page's text is computed from the same run its charts draw (`lib/benchmarks/page.ts`), and the charts are in `components/benchmarks/`. To refresh the snapshot from a local run:

```bash
# in the workers repo, sandbox-libkrun/
target/release/iii-loadtest export target/loadtest/<run> --version <x.y.z>
# here, in website/
pnpm tsx scripts/generate-benchmarks.ts --snapshot --summary <workers>/sandbox-libkrun/target/loadtest/<run>/summary.json \
  --micro <a copy of gh-pages dev/bench/data.js>
```

## Adding a tech spec or a deck

See [roadmap/README.md](./roadmap/README.md). A spec is a `tech-specs/<slug>/` folder of markdown with frontmatter;
it gets a page at `/roadmap/<slug>/` automatically. A deck is `roadmap/<slug>/src/App.tsx`; when present the spec
page links to `/roadmap/<slug>/deck/`.

## Design system

Dark only. Geist Pixel for page and section headings (`components/site/pixel-heading.tsx`), Inter for every label and
sentence, Geist Mono only for code, commands and ids, all through `next/font` and `geist`. Shiki with Vesper and
Vitesse Light for code (`lib/highlight.ts`; markdown uses the same pair). Tokens are the shadcn variables plus
`--line`, `--hero-accent` and the `--motion-*` scale in `app/(site)/globals.css`; the JS twin of the motion scale is
`lib/motion.ts`. Long-form pages (blog, specs, privacy) share `components/article/` and the `.prose` styles.

Add a shadcn component with `pnpm dlx shadcn@latest add <component>` from this directory; it lands in
`components/ui/`.

## Environment

See [`.env.example`](./.env.example). Production builds load GTM, PostHog and Common Room (after cookie consent) and
post email signups to Mailmodo by default. Set `NEXT_PUBLIC_ENABLE_ANALYTICS=false` and
`NEXT_PUBLIC_MAILMODO_FORM_URL=` on preview or demo projects to keep their traffic and test signups out. Live GitHub
and Discord counts need no keys; they are read at build time from public endpoints.

## Deploy

Production: merge to `main`; `deploy-website.yml` builds and syncs `dist/` to S3, updates the KeyValueStore route map,
and invalidates CloudFront. Vercel previews: Root Directory `website`, framework Next.js (set in `vercel.json`),
"Include source files outside of the Root Directory" enabled so `../tech-specs` is available at build time.
