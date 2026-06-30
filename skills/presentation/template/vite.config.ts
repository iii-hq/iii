import { fileURLToPath, URL } from 'node:url'
import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

export default defineConfig({
  // Relative asset paths so the built presentation can be hosted at any subpath
  // (or opened straight from a static file server).
  base: './',
  // the spec viewer bundles the sibling spec markdown (`<spec-dir>/*.md`),
  // which sits just outside this project root — allow the dev server to read it
  server: { fs: { allow: ['..'] } },
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
})
