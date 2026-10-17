import { defineConfig } from 'tsdown'

export default defineConfig({
  entry: ['./src/index.ts', './src/stream.ts', './src/state.ts', './src/helpers.ts'],
  format: ['esm', 'cjs'],
  // The syntax the package promises (tsconfig `target`): newer syntax in the
  // sources, ours or the Node SDK's shared filter (`??=`, numeric separators),
  // is lowered.
  target: 'es2020',
  dts: true,
  sourcemap: true,
  clean: true,
  minify: false,
  treeshake: true,
  deps: { neverBundle: [] },
})
