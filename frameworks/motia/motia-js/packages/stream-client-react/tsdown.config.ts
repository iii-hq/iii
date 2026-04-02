import pluginBabel from '@rollup/plugin-babel'
import { defineConfig } from 'tsdown'

export default defineConfig({
  entry: {
    index: './index.ts',
  },
  format: 'esm',
  platform: 'browser',
  external: ['@motiadev/stream-client-browser', /^react($|\/)/, /^react-dom($|\/)/, 'react/jsx-runtime'],
  dts: {
    build: true,
  },
  clean: true,
  outDir: 'dist',
  exports: {
    devExports: 'development',
  },
  plugins: [
    pluginBabel({
      babelHelpers: 'bundled',
      parserOpts: {
        sourceType: 'module',
        plugins: ['jsx', 'typescript'],
      },
      plugins: ['babel-plugin-react-compiler'],
      extensions: ['.js', '.jsx', '.ts', '.tsx'],
    }),
  ],
})
