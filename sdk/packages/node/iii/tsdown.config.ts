import { defineConfig } from 'tsdown'

export default defineConfig({
  entry: [
    './src/index.ts',
    './src/stream.ts',
    './src/state.ts',
    './src/helpers.ts',
    './src/channel.ts',
    './src/trigger.ts',
    './src/runtime.ts',
    './src/errors.ts',
    './src/internal.ts',
    './src/engine.ts',
  ],
  format: ['esm', 'cjs'],
  dts: true,
  sourcemap: true,
  clean: true,
  minify: false,
  treeshake: true,
  deps: { neverBundle: [] },
})
