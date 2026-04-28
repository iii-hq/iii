# iii.dev website

The iii.dev marketing site. Plain static HTML, no build step.

## Layout

```
website/
├── index.html        # main page (navbar + hero + sections)
├── manifesto.html    # /manifesto page
├── fonts/            # Chivo Mono variable fonts (TTF)
├── package.json      # local dev server
├── vercel.json       # deploy config (clean URLs + /docs proxy)
└── README.md
```

## Local development

From the monorepo root:

```bash
pnpm dev:website
```

Or directly from this directory:

```bash
pnpm install
pnpm dev
```

The site is served at http://localhost:3000.

> Note: `pnpm dev` runs [`serve`](https://www.npmjs.com/package/serve), which honors `vercel.json`'s `cleanUrls` setting, so `/manifesto` resolves to `manifesto.html` locally just as it does in production.

## Deploying to Vercel

The `vercel.json` is set up so Vercel serves this directory as static files with no build:

- `cleanUrls: true` — `/manifesto` serves `manifesto.html`
- `/docs` and `/docs/*` proxy to the docs deployment (`iii-docs.vercel.app`)
- `/api/search` proxies to the docs search endpoint

To deploy, point a Vercel project at this directory (`website/`) with framework preset **Other** and no build command. The default output is the directory itself.

## Editing the site

Just edit `index.html` (or `manifesto.html`) directly. There is no bundler, no React, no Tailwind compile step — all styles are inline `<style>` and all interactivity is inline `<script>`. Refresh the browser to see changes.
