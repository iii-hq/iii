import path from 'path'
import { fileURLToPath } from 'url'
import { defineConfig } from 'vitest/config'

const __dirname = path.dirname(fileURLToPath(import.meta.url))

export default defineConfig({
  resolve: {
    alias: {
      '@iii-dev/helpers/observability/internal': path.resolve(
        __dirname,
        '../helpers/src/observability/internal.ts',
      ),
      '@iii-dev/helpers/observability': path.resolve(__dirname, '../helpers/src/observability/index.ts'),
    },
  },
  test: {
    globals: true,
    testTimeout: 30000,
    hookTimeout: 60000,
    setupFiles: ['./tests/setup.ts'],
    coverage: {
      provider: 'v8',
      include: ['src/**/*.ts'],
      reporter: ['text', 'lcov'],
      reportsDirectory: './coverage',
      exclude: ['src/stream.ts', 'src/triggers.ts', 'src/types.ts'],
      thresholds: {
        lines: 60,
        functions: 60,
        branches: 60,
        statements: 60,
      },
    },
  },
})
