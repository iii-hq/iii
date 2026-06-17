import { defineConfig } from 'tsdown'

export default defineConfig({
  entry: [
    './src/http/index.ts',
    './src/queue/index.ts',
    './src/stream/index.ts',
    './src/worker-connection-manager/index.ts',
    './src/observability/index.ts',
    './src/observability/internal.ts',
  ],
  format: ['esm', 'cjs'],
  dts: true,
  sourcemap: true,
  clean: true,
  minify: false,
  treeshake: true,
  deps: { neverBundle: [] },
})
