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

Not wired up yet. The next step (tracked separately) will:

1. Add a CloudFront cache behavior for `/blog*` that serves `blog/dist/` from
   the existing site S3 bucket under the `blog/` prefix.
2. Update the CloudFront viewer-request function so its SPA fallback does
   not collide with `/blog/<slug>/` URLs.
3. Sync `blog/dist/` to `s3://<site-bucket>/blog/` from CI.
4. Optionally add a Vercel `rewrites` entry for environments that still use
   Vercel.

See `infra/terraform/website/cloudfront.tf` for the current routing.

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
