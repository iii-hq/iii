import { fileURLToPath, URL } from 'node:url'
import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'
import { specManifestPlugin } from './scripts/manifest.mjs'

// One config for every app in this project. `vite` (dev) serves the gallery at
// `/` and every deck at `/<slug>/` from a single server; `build.mjs` reuses
// this file programmatically with a per-deck `root`/`outDir`, so each deck
// ships as a self-contained, relative-path static site that works under any
// URL prefix (deployed at iii.dev/tech-specs/<slug>/).
export default defineConfig({
  base: './',
  plugins: [react(), tailwindcss(), specManifestPlugin()],
  resolve: {
    alias: {
      '@lib': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  // Deck spec pages bundle `../../../../tech-specs/<slug>/*.md`, which sits
  // outside the per-deck root — allow the dev server to read from the repo root.
  server: { fs: { allow: [fileURLToPath(new URL('../..', import.meta.url))] } },
})
