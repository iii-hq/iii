import path from 'path'
import { fileURLToPath } from 'url'
import { defineConfig } from 'vitest/config'

const __dirname = path.dirname(fileURLToPath(import.meta.url))

/**
 * `pnpm bench`: the benchmarks under `bench/`. They run against an in-process
 * fake engine, so there is no setup file and nothing connects to `III_URL`.
 */
export default defineConfig({
  resolve: {
    alias: {
      '@iii-dev/helpers/observability/internal': path.resolve(__dirname, '../helpers/src/observability/internal.ts'),
      '@iii-dev/helpers/observability': path.resolve(__dirname, '../helpers/src/observability/index.ts'),
    },
  },
  test: {
    globals: true,
    include: [],
    benchmark: {
      include: ['bench/**/*.bench.ts'],
    },
  },
})
