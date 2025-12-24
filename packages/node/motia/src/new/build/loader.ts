import * as esbuild from 'esbuild'
import { glob } from 'glob'
import { pathToFileURL } from 'url'
import os from 'os'
import fs from 'fs'
import path from 'path'
import type { Step } from '../../types'
import type { Stream } from '../../types-stream'

export interface LoadedStep {
  config: Step['config']
  filePath: string
  handler: unknown
}

export interface LoadedStream {
  config: Stream['config']
  filePath: string
}

export interface LoadedFiles {
  steps: LoadedStep[]
  streams: Record<string, LoadedStream>
}

export const loadStepsAndStreams = async (): Promise<LoadedFiles> => {
  const stepFiles = await glob('**/*.step.{ts,js}', {
    cwd: process.cwd(),
    ignore: ['node_modules/**', 'dist/**'],
  })
  const streamFiles = await glob('**/*.stream.{ts,js}', {
    cwd: process.cwd(),
    ignore: ['node_modules/**', 'dist/**'],
  })

  const steps = await Promise.all(stepFiles.map((f) => loadFile(f, 'step')))
  const streamResults = await Promise.all(streamFiles.map((f) => loadFile(f, 'stream')))

  return {
    steps: steps.filter(Boolean) as LoadedStep[],
    streams: Object.fromEntries(
      streamResults.filter(Boolean).map((s) => [s!.config.name, s as LoadedStream]),
    ),
  }
}

const loadFile = async (file: string, type: 'step' | 'stream'): Promise<LoadedStep | LoadedStream | null> => {
  try {
    const result = await esbuild.build({
      entryPoints: [file],
      bundle: true,
      platform: 'node',
      format: 'esm',
      write: false,
      external: [],
    })

    const tempFile = path.join(
      os.tmpdir(),
      `motia-${type}-${Date.now()}-${Math.random().toString(36).slice(2)}.mjs`,
    )
    fs.writeFileSync(tempFile, result.outputFiles[0].text)

    const module = await import(pathToFileURL(tempFile).href)
    fs.unlinkSync(tempFile)

    if (type === 'step') {
      return { config: module.config, filePath: file, handler: module.handler }
    } else {
      return { config: module.config, filePath: file }
    }
  } catch (error) {
    console.error(`Failed to load ${file}:`, error)
    return null
  }
}

