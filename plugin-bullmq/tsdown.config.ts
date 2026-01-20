import pluginBabel from '@rollup/plugin-babel'
import postcss from 'rollup-plugin-postcss'
import { defineConfig } from 'tsdown'

export default defineConfig([
  // Main JavaScript/TypeScript build
  {
    entry: {
      index: './src/index.ts',
      plugin: './src/plugin.ts',
    },
    format: 'esm',
    platform: 'browser',
    external: [/^react($|\/)/, 'react/jsx-runtime', '@motiadev/stream-client-react', /^lucide-react($|\/)/],
    dts: {
      build: true,
    },
    exports: {
      devExports: 'development',
    },
    clean: true,
    outDir: 'dist',
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
  },
  // Separate CSS build
  {
    entry: {
      styles: './src/styles.css',
    },
    format: 'esm',
    platform: 'browser',
    outDir: 'dist',
    clean: false,
    plugins: [
      postcss({
        extract: true,
        minimize: process.env.NODE_ENV === 'prod',
      }),
    ],
  },
])
