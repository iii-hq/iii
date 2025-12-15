import * as esbuild from 'esbuild'
import { generateIndex } from './generate-index'

type BuildOptions = {
  external: string[]
}

export const build = (options: BuildOptions) => {
  return esbuild.build({
    stdin: {
      contents: generateIndex(),
      sourcefile: 'index-production.js',
      resolveDir: process.cwd(),
      loader: 'js',
    },
    external: [...options.external, 'ws'],
    platform: 'node',
    target: ['node22'],
    format: 'esm',
    bundle: true,
    minify: true,
    sourcemap: true,
    treeShaking: true,
    outfile: 'dist/index-production.js',
  })
}
