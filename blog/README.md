# iii blog

The iii.dev/blog — static blog built with [Astro](https://astro.build).

## Local development

From the monorepo root:

```bash
pnpm install
pnpm --filter iii-blog dev
```

The dev server runs at <http://localhost:4321/blog/>. Posts live in
`src/content/blog/` as Markdown or MDX with the frontmatter schema defined in
`src/content.config.ts`.

## Build

```bash
pnpm --filter iii-blog build
```

Output is emitted to `blog/dist/` with all routes scoped under `/blog/` thanks
to `base: '/blog'` in `astro.config.mjs`.

## Tests

```bash
pnpm --filter iii-blog test
```

Tests live in `tests/` and run via `node:test` after `astro build`. They
verify the build output (URL scoping, RSS feed, post emission) so regressions
in the base path or content collection setup fail loudly.

## Deployment

The blog ships with the rest of `iii.dev` via
[`.github/workflows/deploy-website.yml`](../.github/workflows/deploy-website.yml).
On every push to `main` that touches `blog/**`, `website/**`, or
`infra/terraform/website/**`, CI:

1. Builds with `pnpm --filter iii-blog build` → `blog/dist/`.
2. Syncs `blog/dist/` to `s3://<site-bucket>/blog/`. Hashed assets get a
   long `immutable` cache; `*.html` and `*.xml` get `must-revalidate`.
3. Invalidates the CloudFront distribution at `/*`.

Routing under `/blog/*` is handled in
[`infra/terraform/website/cloudfront_functions/redirects.js`](../infra/terraform/website/cloudfront_functions/redirects.js)
— see the unit tests there for the exact behavior. In short:

- `/blog` → 301 `/blog/`
- `/blog/<slug>/` → S3 key `blog/<slug>/index.html`
- `/blog/<slug>` → 301 `/blog/<slug>/`
- `/blog/<file.ext>` → pass through

## Adding a post

Create a new file in `src/content/blog/<slug>.md` (or `.mdx`):

```markdown
---
title: 'My new post'
description: 'A short summary used for the index page and RSS feed.'
pubDate: 2026-05-10
tags: ['engine']
---

Post body in Markdown.
```

The slug is the filename minus the extension. The post is published
automatically unless `draft: true` is set in frontmatter.
