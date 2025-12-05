import { defineConfig } from 'tsdown'

export default defineConfig({
  entry: ['./index.ts', './build.ts'],
  format: ['esm', 'cjs'],
  dts: true,
  sourcemap: true,
  clean: true,
  external: [],
  minify: false,
  treeshake: true,
})
