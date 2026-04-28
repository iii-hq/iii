# iii.dev website

The iii.dev site. Plain static HTML, no build step.

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

### Mailmodo

The hero and footer email forms POST to a Mailmodo form endpoint configured via a meta tag at the top of `index.html`:

```html
<meta
  name="iii:mailmodo-form-url"
  content="https://api.mailmodo.com/api/v1/at/f/..."
/>
```

The endpoint is checked into source. Mailmodo form endpoints are public-client-safe (the same URL is what would be embedded in any front-end form), so there's no secret material in this value. To change it per environment:

- **Production**: edit the `content` attribute in `index.html` and redeploy.
- **Locally / per-branch**: edit the same attribute on a feature branch.
- **To disable submission entirely**: clear the `content` attribute. The form will still show the "Thanks for subscribing!" success state and persist to `localStorage`, but no request goes to Mailmodo.

## Editing the site

Just edit `index.html` (or `manifesto.html`) directly. There is no bundler, no React, no Tailwind compile step — all styles are inline `<style>` and all interactivity is inline `<script>`. Refresh the browser to see changes.
