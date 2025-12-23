import { $ } from 'bun'
import { existsSync } from 'fs'
import { watch } from 'fs'
import { join } from 'path'
import { generateIndex } from './generate-index'
import type { Subprocess } from 'bun'

const TEMP_ENTRY = '.motia-dev-entry.ts'
const OUTPUT_FILE = 'dist/index-dev.js'
const WATCH_DIRS = ['steps', 'streams', 'src']
const WATCH_PATTERNS = /\.(step|stream)\.(ts|js)$/

let subprocess: Subprocess | null = null
let debounceTimer: ReturnType<typeof setTimeout> | null = null

const buildAndRun = async (): Promise<Subprocess | null> => {
  const entryPath = join(process.cwd(), TEMP_ENTRY)
  const outdir = join(process.cwd(), 'dist')

  // Write generated index to temp file (Bun.build requires file entrypoints)
  await Bun.write(entryPath, generateIndex())

  try {
    const result = await Bun.build({
      entrypoints: [entryPath],
      outdir,
      naming: 'index-dev.js',
      target: 'node',
      format: 'esm',
      sourcemap: 'linked',
      packages: 'external',
    })

    if (!result.success) {
      console.error('Build failed:')
      for (const log of result.logs) {
        console.error(log)
      }
      return null
    }

    // Clean up temp entry file
    if (await Bun.file(entryPath).exists()) {
      await $`rm ${entryPath}`.quiet()
    }

    // Run the built file
    return Bun.spawn(['bun', '--enable-source-maps', OUTPUT_FILE], {
      cwd: process.cwd(),
      stdout: 'inherit',
      stderr: 'inherit',
    })
  } catch (error) {
    console.error('Build error:', error)
    return null
  }
}

const rebuild = async () => {
  console.log('[watch] Rebuilding...')

  // Kill existing subprocess
  if (subprocess) {
    subprocess.kill()
    subprocess = null
  }

  subprocess = await buildAndRun()
}

export const dev = async () => {
  // Initial build and run
  subprocess = await buildAndRun()

  // Watch for changes in source directories
  for (const dir of WATCH_DIRS) {
    const watchDir = join(process.cwd(), dir)

    if (existsSync(watchDir)) {
      watch(watchDir, { recursive: true }, (event, filename) => {
        if (filename && WATCH_PATTERNS.test(filename)) {
          console.log(`[watch] ${filename} changed`)

          // Debounce to avoid multiple rapid rebuilds
          if (debounceTimer) {
            clearTimeout(debounceTimer)
          }

          debounceTimer = setTimeout(() => {
            rebuild()
          }, 100)
        }
      })

      console.log(`[watch] Watching ${dir}/`)
    }
  }

  // Keep the process alive
  await new Promise(() => {})
}
