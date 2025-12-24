import * as esbuild from 'esbuild'
import { generateIndex } from './generate-index'
import { typegen } from './typegen'

export const dev = async () => {
  const stopTypegen = await typegen({ watch: true, silent: false })

  const result = await esbuild.build({
    stdin: {
      contents: generateIndex(),
      sourcefile: 'index-dev.js',
      resolveDir: process.cwd(),
      loader: 'js',
    },
    packages: 'external',
    platform: 'node',
    target: ['node22'],
    format: 'esm',
    bundle: true,
    sourcemap: true,
    outfile: 'dist/index-dev.js',
  })

  return result
}
