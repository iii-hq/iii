import { defineConfig } from 'tsdown'

export default defineConfig({
  entry: ['./src/index.ts', './src/stream.ts', './src/state.ts', './src/telemetry.ts'],
  format: ['esm', 'cjs'],
  dts: true,
  sourcemap: true,
  clean: true,
  minify: false,
  treeshake: true,
  deps: { neverBundle: [] },
})
