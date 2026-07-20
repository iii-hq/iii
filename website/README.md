# iii.dev website

One [Astro](https://astro.build) project (the Vite-based static-site generator)
that builds **all of iii.dev** into `dist/` — fully static, prerendered HTML,
nothing rendered at request time:

| URL | Source |
| --- | --- |
| `/` | `src/pages/index.astro` — the landing page |
| `/manifesto`, `/privacy-policy` | `src/pages/{manifesto,privacy-policy}.astro` |
| `/blog/`, `/blog/<slug>/`, `/blog/rss.xml` | markdown posts in `src/content/blog/` |
| `/roadmap/`, `/roadmap/<slug>/` | `src/pages/roadmap/` — gallery + one page per tech spec, rendering the deck React islands from `roadmap/` ([roadmap/README.md](./roadmap/README.md)) |
| `/sitemap.xml`, `/llms.txt`, `/AGENTS.md` | generated into `dist/` by `scripts/` at build time |
| `/robots.txt`, `/fonts/*`, `/favicon.svg`, `/og-image.png`, `/posthog-consent.js` | `public/` (copied verbatim) |

## Local development

```bash
pnpm install            # repo root (workspace)
pnpm dev                # THE dev server at :4321 — the whole site, one origin, HMR everywhere
pnpm build              # the full site → dist/  (roadmap contract checks → astro build →
                        #   llms/AGENTS → sitemap)
pnpm preview            # serve dist/ at :4321
pnpm test               # script unit tests + CloudFront function tests (no build needed)
pnpm test:dist          # post-build contract for the whole dist/ tree (run after pnpm build)
pnpm type-check         # strict TS over the roadmap tree (shared lib + every deck)
```

From the repo root, prefix with `pnpm --filter iii-website <script>` (or use
`pnpm dev:website`).

## Writing a blog post

Add a markdown file at `src/content/blog/<slug>.md` — the filename is the URL
(`/blog/<slug>/`):

```yaml
---
title: 'The Harness Is the Backend'
description: 'One-or-two-sentence summary; becomes the meta description and RSS blurb.'
pubDate: 2026-04-28
author: 'Mike Piccolo, Founder & CEO of iii'   # optional
tags: ['agents', 'architecture']               # optional
updatedDate: 2026-05-02                        # optional
draft: true                                    # optional — hides from build, sitemap, RSS
---

Post body in markdown (MDX also works).
```

Images live in `src/assets/blog/<slug>/` and are referenced relatively
(`![alt](../../assets/blog/<slug>/banner.png)`) — Astro hashes and optimizes
them at build. Every post automatically gets a canonical URL, OpenGraph
`article` tags, BlogPosting JSON-LD, the RSS entry, and a sitemap entry.

## Landing page structure

The landing page (and manifesto/privacy) markup is hand-written HTML that
predates Astro. It is kept **byte-faithful** as raw fragments:

- `src/components/landing/sections/*.html` — one file per page section
- `src/components/landing/scripts/*.html` — the page's inline script blocks
- `src/styles/landing.css` — the page stylesheet (built, hashed, cached immutable)
- `src/pages/index.astro` — only assembles the fragments via `set:html`

Edit the fragments directly. They are injected raw, so Astro's template
compiler never reinterprets their braces or tag balance — what's in the file
is what ships. Shared chrome lives once: `src/components/AnalyticsHead.astro`
(consent-gated GTM / Common Room / PostHog), `CookieBanner.astro`,
`ThemeInit.astro`, and the `src/layouts/HtmlShell.astro` head.

## SEO invariants (load-bearing — don't change casually)

- **URL shapes are contracts.** `build.format: 'preserve'` keeps them exact:
  marketing pages are extensionless without trailing slash (`/manifesto` →
  `manifesto.html`, resolved at the edge by the CloudFront KVS route map);
  blog and roadmap pages are directory-style with trailing slash
  (`/blog/<slug>/`, `/roadmap/<slug>/`, rewritten to `…/index.html` by the
  CloudFront redirects function). See `infra/terraform/website/`.
- **Adding a top-level page** is content-only: create `src/pages/foo.astro`,
  link to `/foo` — the deploy syncs the KVS route map from `dist/*.html`
  (`scripts/routes-kvs.ts`); add the path to `scripts/routes.ts` so the
  sitemap lists it.
- Every page emits a canonical URL, meta description, OG/Twitter cards, and
  the shared `og-image.png`; the landing page carries Organization / WebSite /
  SoftwareApplication JSON-LD, blog posts carry BlogPosting JSON-LD.
- `sitemap.xml`, `llms.txt`, and `AGENTS.md` are **generated at build** into
  `dist/` (`scripts/generate-sitemap.ts`, `scripts/generate-llms-agents.ts` —
  the latter scrapes the built homepage and fails the build if the landing
  sections it extracts from disappear).

## Deploying

Merging to `main` runs `.github/workflows/deploy-website.yml`:
`pnpm --filter iii-website build`, two `aws s3 sync`s of `dist/` (immutable
hashed assets vs must-revalidate content files), the CloudFront KVS route-map
sync, then a CloudFront invalidation. No Vercel, no manual steps.

### Mailmodo

The hero and footer email forms POST to a Mailmodo endpoint configured via a
meta tag (`iii:mailmodo-form-url`) in
`src/components/landing/head-extras.html`. The endpoint is public-client-safe
(the same URL any front-end form would embed), so it lives in source. Edit the
`content` attribute and redeploy to change it.
