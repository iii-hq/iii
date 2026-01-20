import { isApiStep, type MotiaPlugin, type MotiaPluginContext, type Step } from '@motiadev/core'
import { EndpointsStream, mapEndpoint } from './endpoints-stream'

export function plugin({ lockedData }: MotiaPluginContext): MotiaPlugin {
  const stream = lockedData.createStream({
    filePath: '__motia.api-endpoints.ts',
    hidden: true,
    config: {
      name: '__motia.api-endpoints',
      baseConfig: { storageType: 'custom', factory: () => new EndpointsStream(lockedData) },
      schema: null as never,
    },
  })()

  const apiStepCreated = (step: Step) => {
    if (isApiStep(step)) {
      stream.set('default', step.filePath, {
        id: step.filePath,
        method: step.config.method,
        path: step.config.path,
        description: step.config.description,
        queryParams: step.config.queryParams,
      })
    }
  }

  const apiStepUpdated = (step: Step) => {
    if (isApiStep(step)) {
      stream.set('default', step.filePath, mapEndpoint(step))
    }
  }

  const apiStepRemoved = (step: Step) => {
    if (isApiStep(step)) {
      stream.delete('default', step.filePath)
    }
  }

  lockedData.onStep('step-created', apiStepCreated)
  lockedData.onStep('step-updated', apiStepUpdated)
  lockedData.onStep('step-removed', apiStepRemoved)

  return {
    workbench: [
      {
        packageName: '@motiadev/plugin-endpoint',
        cssImports: ['@motiadev/plugin-endpoint/dist/styles.css'],
        label: 'Endpoints',
        position: 'top',
        componentName: 'EndpointsPage',
        labelIcon: 'link-2',
      },
    ],
  }
}
