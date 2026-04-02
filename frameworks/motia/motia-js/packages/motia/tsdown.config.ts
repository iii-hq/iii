import { defineConfig } from 'tsdown'

const shared = {
  dts: true,
  sourcemap: true,
  external: [],
  minify: false,
  treeshake: true,
}

export default defineConfig([
  {
    ...shared,
    entry: ['./src/index.ts'],
    format: ['esm', 'cjs'],
    clean: true,
  },
  {
    ...shared,
    entry: { 'new/cli': './src/new/cli.ts' },
    format: ['esm', 'cjs'],
    clean: false,
    banner: {
      js: '#!/usr/bin/env node\n',
    },
  },
])
