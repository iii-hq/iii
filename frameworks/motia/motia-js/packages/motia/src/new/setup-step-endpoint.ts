import fs from 'fs/promises'
import type { ISdk } from 'iii-sdk'
import path from 'path'
import { getStepFilesFromDir } from './build/generate-index'
import { generateStepId } from './build/loader'

const getFeatures = async (filePath: string) => {
  const stat = await fs.stat(`${filePath}-features.json`).catch(() => null)

  if (!stat || stat.isDirectory()) {
    return []
  }

  try {
    const content = await fs.readFile(`${filePath}-features.json`, 'utf8')
    return JSON.parse(content)
  } catch {
    return []
  }
}

export function setupStepEndpoint(iii: ISdk): void {
  const function_id = 'motia_step_get'

  iii.registerFunction({ id: function_id }, async (req) => {
    const id = req.path_params.stepId

    const stepFiles = [
      ...getStepFilesFromDir(path.join(process.cwd(), 'steps')),
      ...getStepFilesFromDir(path.join(process.cwd(), 'src')),
    ]
    const step = stepFiles.find((step) => generateStepId(step) === id)

    if (!step) {
      return {
        status_code: 404,
        body: { error: 'Step not found' },
      }
    }

    try {
      const content = await fs.readFile(step, 'utf8')
      const features = await getFeatures(step.replace(`${path.sep}src${path.sep}`, `${path.sep}tutorial${path.sep}`))

      return {
        status_code: 200,
        body: { id, content, features },
      }
    } catch (error) {
      console.error('Error reading step file:', error)
      return {
        status_code: 500,
        body: { error: 'Failed to read step file' },
      }
    }
  })

  iii.registerTrigger({
    type: 'http',
    function_id,
    config: { api_path: '__motia/step/:stepId', http_method: 'GET', description: 'Get a step' },
  })
}
