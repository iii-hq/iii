import { defineConfig } from 'bunup'

export default defineConfig([
  {
    name: 'cli',
    entry: ['./src/new/cli.ts'],
    format: ['esm'],
    clean: true,
    minify: false,
    compile: true
  },
  {
    name: 'index',
    entry: ['./src/index.ts'],
    format: ['esm', 'cjs'],
    clean: true,
    minify: false,
  }
])

