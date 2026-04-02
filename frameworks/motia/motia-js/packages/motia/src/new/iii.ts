import { existsSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import type { InitOptions, ISdk } from 'iii-sdk'
import { registerWorker } from 'iii-sdk'

type OtelConfig = NonNullable<InitOptions['otel']>

const engineWsUrl = process.env.III_URL ?? 'ws://localhost:49134'

function readProjectName(): string | undefined {
  const maxDepth = 1
  let dir = process.cwd()
  for (let i = 0; i <= maxDepth; i++) {
    const pkgPath = join(dir, 'package.json')
    if (existsSync(pkgPath)) {
      try {
        const pkg = JSON.parse(readFileSync(pkgPath, 'utf-8'))
        if (typeof pkg.name === 'string' && pkg.name) {
          return pkg.name
        }
      } catch {
        // ignore
      }
    }
    const parent = dirname(dir)
    if (parent === dir) break
    dir = parent
  }
  return undefined
}

const createIII = (otelConfig?: Partial<OtelConfig>) => {
  return registerWorker(engineWsUrl, {
    otel: {
      enabled: true,
      serviceName: 'motia',
      ...otelConfig,
    },
    telemetry: {
      framework: 'motia',
      project_name: readProjectName(),
    },
  })
}

let instance: ISdk | undefined

export const getInstance = (): ISdk => {
  if (!instance) {
    instance = createIII()
  }
  return instance
}

export const initIII = (otelConfig?: Partial<OtelConfig>) => {
  instance = createIII(otelConfig)
  return instance
}
