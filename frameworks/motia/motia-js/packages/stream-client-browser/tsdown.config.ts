import { defineConfig } from 'tsdown'

export default defineConfig({
  entry: {
    index: './index.ts',
  },
  format: 'esm',
  platform: 'browser',
  external: ['@motiadev/stream-client', 'uuid'],
  dts: {
    build: true,
  },
  clean: true,
  outDir: 'dist',
  exports: {
    devExports: 'development',
  },
})
