import { bridge } from './bridge'
import {
  type ApiResponse,
  getContext,
  type TriggerCondition,
  type TriggerInput,
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

async function singleConditionHandler(input: TriggerInput): Promise<void> {
  const { logger } = getContext()
  logger.info('Processing high-value order', { data: input.data })
}

bridge.registerFunction({ function_path: 'single-condition-test' }, singleConditionHandler)
bridge.registerTrigger({
  function_path: 'single-condition-test',
  triggers: [
    {
      trigger_type: 'event',
      config: { topic: 'order.created' },
      conditions: [isHighValue],
    },
  ],
})

async function multipleConditionsHandler(input: TriggerInput): Promise<void> {
  const { logger } = getContext()
  logger.info('Processing premium order', { data: input.data })
}

bridge.registerFunction({ function_path: 'multiple-conditions-test' }, multipleConditionsHandler)
bridge.registerTrigger({
  function_path: 'multiple-conditions-test',
  triggers: [
    {
      trigger_type: 'event',
      config: { topic: 'order.created' },
      conditions: [isHighValue, isVerifiedUser, isDomestic],
    },
  ],
})

async function apiWithConditionsHandler(input: TriggerInput): Promise<ApiResponse> {
  const { logger } = getContext()
  logger.info('Processing premium order via API')
  return {
    status_code: 200,
    body: { message: 'Premium order processed', data: input.data },
  }
}

bridge.registerFunction({ function_path: 'api-with-conditions' }, apiWithConditionsHandler)
bridge.registerTrigger({
  function_path: 'api-with-conditions',
  triggers: [
    {
      trigger_type: 'api',
      config: { api_path: 'orders/premium', http_method: 'POST' },
      conditions: [isHighValue, isVerifiedUser],
    },
  ],
})

const isBusinessHours: TriggerCondition = (input, ctx, trigger) => {
  const now = new Date()
  const hour = now.getHours()
  return hour >= 9 && hour < 17
}

async function cronWithConditionHandler(input: TriggerInput): Promise<void> {
  const { logger } = getContext()
  logger.info('Running business hours task')
}

bridge.registerFunction({ function_path: 'cron-with-condition' }, cronWithConditionHandler)
bridge.registerTrigger({
  function_path: 'cron-with-condition',
  triggers: [
    {
      trigger_type: 'cron',
      config: { expression: '*/5 * * * *' },
      conditions: [isBusinessHours],
    },
  ],
})

async function mixedTriggersHandler(input: TriggerInput): Promise<any> {
  const { logger } = getContext()
  logger.info('Processing order', { data: input.data, trigger: input.trigger.type })

  if (input.trigger.type === 'api') {
    return { status_code: 200, body: { message: 'Order processed via API' } }
  }

  return undefined
}

bridge.registerFunction({ function_path: 'mixed-triggers-with-conditions' }, mixedTriggersHandler)
bridge.registerTrigger({
  function_path: 'mixed-triggers-with-conditions',
  triggers: [
    {
      trigger_type: 'event',
      config: { topic: 'order.created' },
      conditions: [isHighValue],
    },
    {
      trigger_type: 'api',
      config: { api_path: 'orders/manual', http_method: 'POST' },
      conditions: [isVerifiedUser],
    },
  ],
})
