import { writeFileSync, existsSync } from 'fs'
import { watch } from 'fs'
import { generateTypesString, generateTypesFromSteps, generateTypesFromStreams } from '../../types/generate-types'
import { loadStepsAndStreams } from './loader'
import { Printer } from '../../printer'

export interface TypegenOptions {
  watch?: boolean
  output?: string
  silent?: boolean
}

export const typegen = async (options: TypegenOptions = {}): Promise<() => void> => {
  const output = options.output ?? 'types.d.ts'
  const printer = new Printer(process.cwd())

  await generateTypes(output, printer)

  if (options.watch) {
    return watchFiles(() => generateTypes(output, printer))
  }

  return () => {}
}

const generateTypes = async (output: string, printer: Printer) => {
  try {
    const { steps, streams } = await loadStepsAndStreams()

    const handlersMap = generateTypesFromSteps(steps, printer)
    const streamsMap = generateTypesFromStreams(streams)
    const content = generateTypesString(handlersMap, streamsMap)

    writeFileSync(output, content)
  } catch (error) {
    console.error(`Type generation failed: ${error}`)
  }
}

const watchFiles = (callback: () => void): () => void => {
  const dirs = ['steps', 'streams', 'src'].filter((d) => existsSync(d))
  const watchers: ReturnType<typeof watch>[] = []
  let timeout: NodeJS.Timeout | null = null

  const debouncedCallback = () => {
    if (timeout) clearTimeout(timeout)
    timeout = setTimeout(callback, 100)
  }

  for (const dir of dirs) {
    const watcher = watch(dir, { recursive: true }, (_, filename) => {
      if (filename?.match(/\.(step|stream)\.(ts|js)$/)) {
        debouncedCallback()
      }
    })
    watchers.push(watcher)
  }

  return () => {
    for (const w of watchers) {
      w.close()
    }
    if (timeout) clearTimeout(timeout)
  }
}

