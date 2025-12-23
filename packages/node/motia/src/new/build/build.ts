import { $ } from 'bun'
import { join } from 'path'
import { generateIndex } from './generate-index'

const TEMP_ENTRY = '.motia-build-entry.ts'

type BuildOptions = {
  external: string[]
}

export const build = async (options: BuildOptions) => {
  const entryPath = join(process.cwd(), TEMP_ENTRY)
  const outdir = join(process.cwd(), 'dist')

  // Write generated index to temp file (Bun.build requires file entrypoints)
  await Bun.write(entryPath, generateIndex())

  try {
    const result = await Bun.build({
      entrypoints: [entryPath],
      outdir,
      naming: 'index-production.js',
      target: 'node',
      format: 'esm',
      minify: true,
      sourcemap: 'linked',
      packages: 'external',
      external: [...options.external, 'ws'],
    })

    if (!result.success) {
      console.error('Build failed:')
      for (const log of result.logs) {
        console.error(log)
      }
      process.exit(1)
    }
  } finally {
    // Clean up temp entry file
    if (await Bun.file(entryPath).exists()) {
      await $`rm ${entryPath}`.quiet()
    }
  }
}
