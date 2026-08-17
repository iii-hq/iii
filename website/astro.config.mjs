import { fileURLToPath } from 'node:url'
import mdx from '@astrojs/mdx'
import react from '@astrojs/react'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig } from 'astro/config'
import { specManifestPlugin } from './roadmap/scripts/manifest.mjs'

// One static site for all of iii.dev — one dev server, one build — deployed
// to S3 + CloudFront by .github/workflows/deploy-website.yml. URL shapes are
// load-bearing SEO contracts enforced by build.format 'preserve':
//   /            ← src/pages/index.astro            → dist/index.html
//   /manifesto   ← src/pages/manifesto.astro        → dist/manifesto.html
//                  (extensionless URL resolved by the CloudFront KVS route map)
//   /blog/<x>/   ← src/pages/blog/[...slug]/index.astro → dist/blog/<x>/index.html
//                  (directory URL resolved by the CloudFront redirects function)
//   /roadmap/*   ← src/pages/roadmap/* — the gallery and each tech-spec deck
//                  as React islands over the shared library in roadmap/src
//                  (@lib), specs discovered via roadmap/scripts/manifest.mjs
// Changing the format changes every canonical URL — don't.
export default defineConfig({
  site: 'https://iii.dev',
  trailingSlash: 'ignore',
  // The landing page ships hand-tuned markup; keep output whitespace as
  // authored so the built HTML stays diffable against its source fragments.
  compressHTML: false,
  build: {
    format: 'preserve',
  },
  markdown: {
    syntaxHighlight: 'shiki',
    shikiConfig: {
      theme: 'github-dark-dimmed',
      wrap: false,
    },
  },
  integrations: [mdx(), react()],
  vite: {
    // Tailwind processes roadmap/src/index.css; the spec-manifest plugin
    // resolves virtual:spec-manifest (the gallery's data source) and reloads
    // on tech-specs/ edits in dev.
    plugins: [tailwindcss(), specManifestPlugin()],
    // roadmap/src/content/mermaid.tsx lazy-imports mermaid; pre-bundle it so
    // the dev-server dynamic import never 404s a not-yet-optimized chunk
    // (spec diagrams would silently fall back to their source text).
    optimizeDeps: { include: ['mermaid'] },
    resolve: {
      alias: {
        '@lib': fileURLToPath(new URL('./roadmap/src', import.meta.url)),
      },
    },
    // Deck spec pages bundle ../../../../tech-specs/<slug>/*.md, which sits
    // outside the website root — let the dev server read from the repo root.
    server: { fs: { allow: [fileURLToPath(new URL('..', import.meta.url))] } },
  },
})
