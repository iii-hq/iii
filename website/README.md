# iii.dev website

One [Next.js](https://nextjs.org) app (App Router, static export) that builds **all of iii.dev** into `dist/`:
prerendered HTML, nothing rendered at request time. Deployed to S3 + CloudFront by
`.github/workflows/deploy-website.yml`; Vercel builds previews from the same `pnpm build`.

| URL | Source |
| --- | --- |
| `/` | `src/app/(site)/page.tsx` and `src/components/landing/` |
| `/manifesto`, `/privacy-policy` | `src/app/(site)/{manifesto,privacy-policy}/` (copy in the sibling `*-data.ts`) |
| `/blog/`, `/blog/<slug>/`, `/blog/rss.xml`, `/blog/<slug>.md`, `/blog/index.md` | markdown posts in `src/content/blog/`; the `.md` twins are written by `scripts/generate-blog-md.ts` |
| `/roadmap/`, `/roadmap/<slug>/`, `/roadmap/<slug>/<file>.md`, `/roadmap/index.json` | tech specs read from `../tech-specs/` (markdown only, frontmatter in each `README.md`) |
| `/roadmap/<slug>/deck/` | the interactive presentation for a spec, from `roadmap/<slug>/src/App.tsx` ([roadmap/README.md](./roadmap/README.md)) |
| `/sitemap.xml`, `/llms.txt`, `/AGENTS.md` | generated into `dist/` by `scripts/` after the Next build |
| `/robots.txt`, `/favicon.svg`, `/og-image.png`, `/posthog-consent.js`, `/blog/<slug>/*`, `/console-demo/*`, `/fonts/*` | `public/` (copied verbatim) |

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

## Writing a blog post

Add a markdown file at `src/content/blog/<slug>.md`; the filename is the URL (`/blog/<slug>/`). Frontmatter:

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

Images: put the files in `public/blog/<slug>/` and reference them as `../../assets/blog/<slug>/<file>` in the
markdown (the historical path, rewritten at render time and in the `.md` export).

## Adding a tech spec or a deck

See [roadmap/README.md](./roadmap/README.md). A spec is a `tech-specs/<slug>/` folder of markdown with frontmatter;
it gets a page at `/roadmap/<slug>/` automatically. A deck is `roadmap/<slug>/src/App.tsx`; when present the spec
page links to `/roadmap/<slug>/deck/`.

## Design system

Black and white, dark first. Inter (UI) and Geist Mono (code) through `next/font`; Shiki with Vesper and Min Light
for code. Tokens are the `--gray-1…12` scale in `src/app/(site)/globals.css` (light follows shadcn neutral). Dark mode is
the `.dark` class on `<html>`, set before first paint from the `iii_theme` localStorage key. Motion runs through
`LazyMotion` (`m.*` components only). shadcn/ui components live in `src/components/ui/`; add one with
`pnpm dlx shadcn@latest add <component>` from this directory.

## Environment

See [`.env.example`](./.env.example). Production builds load GTM, PostHog and Common Room (after cookie consent) and
post email signups to Mailmodo by default, as the previous site did. Set `NEXT_PUBLIC_ENABLE_ANALYTICS=false` and
`NEXT_PUBLIC_MAILMODO_FORM_URL=` on preview or demo projects to keep their traffic and test signups out.

## Deploy

Production: merge to `main`; `deploy-website.yml` builds and syncs `dist/` to S3, updates the KeyValueStore route map,
and invalidates CloudFront. Vercel previews: Root Directory `website`, framework Next.js (set in `vercel.json`),
"Include source files outside of the Root Directory" enabled so `../tech-specs` is available at build time.
