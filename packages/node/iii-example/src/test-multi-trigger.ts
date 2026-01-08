import { bridge } from './bridge'
import {
  type ApiResponse,
  type ApiRequest,
  getContext,
  registerStep,
  type StepConfig,
} from '@iii-dev/sdk'

const singleEventConfig: StepConfig = {
  name: 'single-event-trigger',
  triggers: [
    {
      type: 'event',
      subscribes: ['test.event'],
    },
  ],
  emits: ['test.processed'],
  description: 'Test single event trigger',
}

async function singleEventHandler(data: any): Promise<void> {
  const { logger } = getContext()
  logger.info('Single event trigger fired', { data })
}

bridge.registerFunction({ function_path: singleEventConfig.name }, singleEventHandler)
registerStep(bridge, singleEventConfig, singleEventConfig.name)

const singleApiConfig: StepConfig = {
  name: 'single-api-trigger',
  triggers: [
    {
      type: 'api',
      path: '/test/single',
      method: 'GET',
    },
  ],
  emits: [],
  description: 'Test single API trigger',
}

async function singleApiHandler(req: ApiRequest<any>): Promise<ApiResponse> {
  const { logger } = getContext()
  logger.info('Single API trigger fired')
  return { status_code: 200, body: { message: 'Single API trigger works' } }
}

bridge.registerFunction({ function_path: singleApiConfig.name }, singleApiHandler)
registerStep(bridge, singleApiConfig, singleApiConfig.name)

const singleCronConfig: StepConfig = {
  name: 'single-cron-trigger',
  triggers: [
    {
      type: 'cron',
      expression: '*/5 * * * *',
    },
  ],
  emits: [],
  description: 'Test single cron trigger',
}

async function singleCronHandler(data: any): Promise<void> {
  const { logger } = getContext()
  logger.info('Single cron trigger fired')
}

bridge.registerFunction({ function_path: singleCronConfig.name }, singleCronHandler)
registerStep(bridge, singleCronConfig, singleCronConfig.name)

const dualTriggerConfig: StepConfig = {
  name: 'dual-trigger',
  triggers: [
    {
      type: 'event',
      subscribes: ['test.dual'],
    },
    {
      type: 'api',
      path: '/test/dual',
      method: 'POST',
    },
  ],
  emits: ['test.dual.processed'],
  description: 'Test dual trigger (event + API)',
}

async function dualTriggerHandler(data: any): Promise<any> {
  const { logger } = getContext()
  logger.info('Dual trigger fired', { data })

  if (data && typeof data === 'object' && 'body' in data) {
    return { status_code: 200, body: { message: 'Dual trigger via API' } }
  }
  return undefined
}

bridge.registerFunction({ function_path: dualTriggerConfig.name }, dualTriggerHandler)
registerStep(bridge, dualTriggerConfig, dualTriggerConfig.name)

const tripleTriggerConfig: StepConfig = {
  name: 'triple-trigger',
  triggers: [
    {
      type: 'event',
      subscribes: ['test.triple'],
    },
    {
      type: 'api',
      path: '/test/triple',
      method: 'POST',
    },
    {
      type: 'cron',
      expression: '0 */2 * * *',
    },
  ],
  emits: ['test.triple.processed'],
  description: 'Test triple trigger (event + API + cron)',
}

async function tripleTriggerHandler(data: any): Promise<any> {
  const { logger } = getContext()
  logger.info('Triple trigger fired', { data })

  if (data && typeof data === 'object' && 'body' in data) {
    return { status_code: 200, body: { message: 'Triple trigger via API' } }
  }
  return undefined
}

bridge.registerFunction({ function_path: tripleTriggerConfig.name }, tripleTriggerHandler)
registerStep(bridge, tripleTriggerConfig, tripleTriggerConfig.name)

const multipleEventsConfig: StepConfig = {
  name: 'multiple-events',
  triggers: [
    {
      type: 'event',
      subscribes: ['test.event.1'],
    },
    {
      type: 'event',
      subscribes: ['test.event.2'],
    },
    {
      type: 'event',
      subscribes: ['test.event.3'],
    },
  ],
  emits: ['test.events.processed'],
  description: 'Test multiple event triggers',
}

async function multipleEventsHandler(data: any): Promise<void> {
  const { logger } = getContext()
  logger.info('Multiple events trigger fired', { data })
}

bridge.registerFunction({ function_path: multipleEventsConfig.name }, multipleEventsHandler)
registerStep(bridge, multipleEventsConfig, multipleEventsConfig.name)

const multipleApisConfig: StepConfig = {
  name: 'multiple-apis',
  triggers: [
    {
      type: 'api',
      path: '/test/api/1',
      method: 'GET',
    },
    {
      type: 'api',
      path: '/test/api/2',
      method: 'POST',
    },
    {
      type: 'api',
      path: '/test/api/3',
      method: 'PUT',
    },
  ],
  emits: [],
  description: 'Test multiple API triggers',
}

async function multipleApisHandler(req: ApiRequest<any>): Promise<ApiResponse> {
  const { logger } = getContext()
  logger.info('Multiple APIs trigger fired', { method: req.method })
  return { status_code: 200, body: { message: 'Multiple APIs trigger works' } }
}

bridge.registerFunction({ function_path: multipleApisConfig.name }, multipleApisHandler)
registerStep(bridge, multipleApisConfig, multipleApisConfig.name)
