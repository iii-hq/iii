import { defineConfig } from 'vitest/config'

// Standalone config (does not extend vite.config.ts) so unit tests run without
// the app's Vite plugins. Tests mock network/IO and target node by default.
export default defineConfig({
  test: {
    environment: 'node',
    include: ['src/**/*.test.ts'],
  },
})
