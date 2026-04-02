import * as esbuild from 'esbuild'
import fs from 'fs'
import { glob } from 'glob'
import os from 'os'
import path from 'path'
import { pathToFileURL } from 'url'
import { v5 as uuidv5 } from 'uuid'
import type { Step } from '../../types'
import type { Stream } from '../../types-stream'

export interface LoadedStep {
  id: string
  config: Step['config']
  filePath: string
  handler: unknown
}

export interface LoadedStream {
  id: string
  config: Stream['config']
  filePath: string
}

export interface LoadedFiles {
  steps: LoadedStep[]
  streams: Record<string, LoadedStream>
}

export const STEP_NAMESPACE = '7f1c3ff2-9b00-4d0a-bdd7-efb8bca49d4f'
export const generateStepId = (filePath: string): string => {
  return uuidv5(filePath, STEP_NAMESPACE)
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
    streams: Object.fromEntries(streamResults.filter(Boolean).map((s) => [s?.config?.name, s as LoadedStream])),
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

    const tempFile = path.join(os.tmpdir(), `motia-${type}-${Date.now()}-${Math.random().toString(36).slice(2)}.mjs`)
    fs.writeFileSync(tempFile, result.outputFiles[0].text)

    const module = await import(pathToFileURL(tempFile).href)
    fs.unlinkSync(tempFile)

    if (type === 'step') {
      return { config: module.config, filePath: file, handler: module.handler, id: generateStepId(file) }
    } else {
      return { config: module.config, filePath: file, id: generateStepId(file) }
    }
  } catch (_error) {
    return null
  }
}
