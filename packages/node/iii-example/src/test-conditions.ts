import { bridge } from './bridge'
import {
  type ApiResponse,
  getContext,
  registerStep,
  type StepConfig,
  type TriggerCondition,
  type TriggerInput,
  createConditionWrapper,
} from '@iii-dev/sdk'

const isHighValue: TriggerCondition = (input: TriggerInput, ctx, trigger) => {
  const data = input.data || {}
  const amount = data?.amount || 0
  return amount > 1000
}

const isVerifiedUser: TriggerCondition = (input: TriggerInput, ctx, trigger) => {
  const data = input.data || {}
  const user = data?.user || {}
  return user.verified === true
}

const isDomestic: TriggerCondition = (input: TriggerInput, ctx, trigger) => {
  const data = input.data || {}
  const country = data?.country
  return ['US', 'CA'].includes(country)
}

const singleConditionConfig: StepConfig = {
  name: 'single-condition-test',
  triggers: [
    {
      type: 'event',
      subscribes: ['order.created'],
      conditions: [isHighValue],
    },
  ],
  emits: ['order.processed'],
  description: 'Test single condition on event trigger',
}

async function singleConditionHandler(input: TriggerInput): Promise<void> {
  const { logger } = getContext()
  logger.info('Processing high-value order', { data: input.data })
}

const wrappedSingleHandler = createConditionWrapper(
  singleConditionHandler,
  singleConditionConfig.triggers[0].conditions,
  { type: 'event', index: 0 }
)

bridge.registerFunction({ function_path: singleConditionConfig.name }, wrappedSingleHandler)
registerStep(bridge, singleConditionConfig, singleConditionConfig.name)

const multipleConditionsConfig: StepConfig = {
  name: 'multiple-conditions-test',
  triggers: [
    {
      type: 'event',
      subscribes: ['order.created'],
      conditions: [isHighValue, isVerifiedUser, isDomestic],
    },
  ],
  emits: ['premium.order.processed'],
  description: 'Test multiple conditions (AND logic)',
}

async function multipleConditionsHandler(input: TriggerInput): Promise<void> {
  const { logger } = getContext()
  logger.info('Processing premium order', { data: input.data })
}

const wrappedMultipleHandler = createConditionWrapper(
  multipleConditionsHandler,
  multipleConditionsConfig.triggers[0].conditions,
  { type: 'event', index: 0 }
)

bridge.registerFunction({ function_path: multipleConditionsConfig.name }, wrappedMultipleHandler)
registerStep(bridge, multipleConditionsConfig, multipleConditionsConfig.name)

const apiWithConditionsConfig: StepConfig = {
  name: 'api-with-conditions',
  triggers: [
    {
      type: 'api',
      path: '/orders/premium',
      method: 'POST',
      conditions: [isHighValue, isVerifiedUser],
    },
  ],
  emits: [],
  description: 'Test conditions on API trigger',
}

async function apiWithConditionsHandler(input: TriggerInput): Promise<ApiResponse> {
  const { logger } = getContext()
  logger.info('Processing premium order via API')
  return {
    status_code: 200,
    body: { message: 'Premium order processed', data: input.data },
  }
}

const wrappedApiHandler = createConditionWrapper(
  apiWithConditionsHandler,
  apiWithConditionsConfig.triggers[0].conditions,
  { type: 'api', index: 0 }
)

bridge.registerFunction({ function_path: apiWithConditionsConfig.name }, wrappedApiHandler)
registerStep(bridge, apiWithConditionsConfig, apiWithConditionsConfig.name)

const isBusinessHours: TriggerCondition = (input, ctx, trigger) => {
  const now = new Date()
  const hour = now.getHours()
  return hour >= 9 && hour < 17
}

const cronWithConditionConfig: StepConfig = {
  name: 'cron-with-condition',
  triggers: [
    {
      type: 'cron',
      expression: '*/5 * * * *',
      conditions: [isBusinessHours],
    },
  ],
  emits: [],
  description: 'Test condition on cron trigger',
}

async function cronWithConditionHandler(input: TriggerInput): Promise<void> {
  const { logger } = getContext()
  logger.info('Running business hours task')
}

const wrappedCronHandler = createConditionWrapper(
  cronWithConditionHandler,
  cronWithConditionConfig.triggers[0].conditions,
  { type: 'cron', index: 0 }
)

bridge.registerFunction({ function_path: cronWithConditionConfig.name }, wrappedCronHandler)
registerStep(bridge, cronWithConditionConfig, cronWithConditionConfig.name)

const mixedTriggersConfig: StepConfig = {
  name: 'mixed-triggers-with-conditions',
  triggers: [
    {
      type: 'event',
      subscribes: ['order.created'],
      conditions: [isHighValue],
    },
    {
      type: 'api',
      path: '/orders/manual',
      method: 'POST',
      conditions: [isVerifiedUser],
    },
  ],
  emits: ['order.processed'],
  description: 'Test multiple triggers each with different conditions',
}

async function mixedTriggersHandler(input: TriggerInput): Promise<any> {
  const { logger } = getContext()
  logger.info('Processing order', { data: input.data, trigger: input.trigger.type })

  if (input.trigger.type === 'api') {
    return { status_code: 200, body: { message: 'Order processed via API' } }
  }

  return undefined
}

mixedTriggersConfig.triggers.forEach((trigger, index) => {
  const functionPath = `${mixedTriggersConfig.name}:trigger:${index}`
  const wrappedHandler = createConditionWrapper(mixedTriggersHandler, trigger.conditions, {
    type: trigger.type,
    index,
  })

  bridge.registerFunction({ function_path: functionPath }, wrappedHandler)

  bridge.registerTrigger({
    trigger_type: trigger.type,
    function_path: functionPath,
    config:
      trigger.type === 'event'
        ? { topic: trigger.subscribes[0] }
        : { api_path: trigger.path, http_method: trigger.method },
  })
})
