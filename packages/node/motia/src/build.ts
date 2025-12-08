import * as esbuild from 'esbuild'
import { existsSync } from 'fs'
import { globSync } from 'glob'
import path from 'path'

export const getStreamFilesFromDir = (dir: string): string[] => {
  if (!existsSync(dir)) {
    return []
  }
  return globSync('**/*.stream.{ts,js}', { absolute: true, cwd: dir }).map((file) => file.replace(process.cwd(), '.'))
}

export const getStepFilesFromDir = (dir: string): string[] => {
  if (!existsSync(dir)) {
    return []
  }
  return globSync('**/*.step.{ts,js}', { absolute: true, cwd: dir }).map((file) => file.replace(process.cwd(), '.'))
}

const toSnakeCaseConst = (filePath: string) => {
  // Get file path relative to cwd to not have disk-specific prefixes
  let relPath = path.relative(process.cwd(), filePath).replace(/\\/g, '/')
  // Remove extension
  relPath = relPath.replace(/\.[^/.]+$/, '')
  // Replace invalid JS identifier chars with underscore
  let identifier = relPath.replace(/[^a-zA-Z0-9]+/g, '_')
  // Remove leading/trailing underscores
  identifier = identifier.replace(/^_+|_+$/g, '')
  // To lower case
  return identifier.toLowerCase()
}

const generateIndex = () => {
  const streamsFiles = [
    ...getStreamFilesFromDir(path.join(process.cwd(), 'streams')),
    ...getStreamFilesFromDir(path.join(process.cwd(), 'src')),
    ...getStreamFilesFromDir(path.join(process.cwd(), 'steps')),
  ]

  const streams = streamsFiles.map((file) => {
    const constName = toSnakeCaseConst(file)

    return {
      importStatement: `import * as ${constName} from '${file}';`,
      content: `;(() => {
  const stream = streamWrapper(${constName}.config, '${file}');
  streams[stream.streamName] = stream;
})();`,
    }
  })

  const stepFiles = [
    ...getStepFilesFromDir(path.join(process.cwd(), 'steps')),
    ...getStepFilesFromDir(path.join(process.cwd(), 'src')),
  ]

  const steps = stepFiles.map((file) => {
    const constName = toSnakeCaseConst(file)

    return {
      importStatement: `import * as ${constName} from '${file}';`,
      content: `stepWrapper(${constName}.config, '${file}', ${constName}.handler, streams);`,
    }
  })

  return [
    "import { stepWrapper, streamWrapper } from '@iii-dev/motia'",
    ...streams.map((stream) => stream.importStatement),
    ...steps.map((step) => step.importStatement),
    '',
    'const streams = {}',
    ...streams.map((stream) => stream.content),

    '',
    ...steps.map((step) => step.content),
  ].join('\n')
}

export const build = () => {
  return esbuild.build({
    stdin: {
      contents: generateIndex(),
      sourcefile: 'index.js',
      resolveDir: process.cwd(),
      loader: 'js',
    },
    packages: 'external',
    platform: 'node',
    target: ['node22'],
    format: 'esm',
    bundle: true,
    outfile: 'dist/index.js',
  })
}

build().catch(console.error)
