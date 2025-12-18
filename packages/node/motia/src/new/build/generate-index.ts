import * as esbuild from 'esbuild'
import { existsSync, readFileSync } from 'fs'
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

export const generateIndex = () => {
  const motiaConfigPath = path.join(process.cwd(), 'motia.config.ts')
  const hasMotiaConfig = existsSync(motiaConfigPath)
  const hasAuthenticateStream =
    hasMotiaConfig && readFileSync(motiaConfigPath, 'utf8').includes('export const authenticateStream')

  const streamsFiles = [
    ...getStreamFilesFromDir(path.join(process.cwd(), 'streams')),
    ...getStreamFilesFromDir(path.join(process.cwd(), 'src')),
    ...getStreamFilesFromDir(path.join(process.cwd(), 'steps')),
  ]

  const streams = streamsFiles.map((file) => {
    const constName = toSnakeCaseConst(file)

    return {
      importStatement: `import * as ${constName} from '${file}';`,
      content: `motia.addStream(${constName}.config, '${file}')`,
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
      content: `motia.addStep(${constName}.config, '${file}', ${constName}.handler);`,
    }
  })

  return [
    "import { Motia } from '@iii-dev/motia'",
    hasMotiaConfig ? `import * as motiaConfig from './motia.config';` : '// No motia.config.ts found',

    ...streams.map((stream) => stream.importStatement),
    ...steps.map((step) => step.importStatement),
    '',
    'const motia = new Motia();',
    ...streams.map((stream) => stream.content),

    '',
    ...steps.map((step) => step.content),

    hasMotiaConfig && hasAuthenticateStream
      ? `motia.authenticateStream = motiaConfig.authenticateStream;`
      : '// No authenticateStream found in motia.config.ts',

    'motia.initialize();',
  ].join('\n')
}
