import { defineConfig } from 'tsdown'

export default defineConfig({
  entry: {
    index: './index.ts',
  },
  format: 'esm',
  platform: 'neutral',
  external: ['uuid'],
  dts: {
    build: true,
  },
  clean: true,
  outDir: 'dist',
  exports: {
    devExports: 'development',
  },
})
