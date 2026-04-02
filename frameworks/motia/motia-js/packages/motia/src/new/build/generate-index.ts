import { existsSync, readFileSync } from 'fs'
import { globSync } from 'glob'
import path from 'path'

const toRelativePosix = (file: string): string => {
  return './' + path.relative(process.cwd(), file).replace(/\\/g, '/')
}

export const getStreamFilesFromDir = (dir: string): string[] => {
  if (!existsSync(dir)) {
    return []
  }
  return globSync('**/*.stream.{ts,js}', { absolute: true, cwd: dir }).map(toRelativePosix)
}

export const getStepFilesFromDir = (dir: string): string[] => {
  if (!existsSync(dir)) {
    return []
  }
  return globSync('**/*.step.{ts,js}', { absolute: true, cwd: dir }).map(toRelativePosix)
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
  const motiaConfigContent = hasMotiaConfig ? readFileSync(motiaConfigPath, 'utf8') : ''
  const hasAuthenticateStream = motiaConfigContent.includes('export const authenticateStream')
  const hasOtelConfig = motiaConfigContent.includes('export const otel')

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
      content: `motia.addStep(${constName}.config, '${file}', ${constName}.handler, '${file}');`,
    }
  })

  return [
    "import { Motia, initIII } from 'motia'",
    hasMotiaConfig ? `import * as motiaConfig from './motia.config';` : '// No motia.config.ts found',

    ...streams.map((stream) => stream.importStatement),
    ...steps.map((step) => step.importStatement),
    '',
    hasOtelConfig ? 'initIII(motiaConfig.otel);' : 'initIII();',
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
