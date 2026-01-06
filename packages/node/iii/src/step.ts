import type { BridgeClient, StepConfig, TriggerConfig, Trigger, TriggerCondition, TriggerInfo } from './types'

export function registerStep(bridge: BridgeClient, config: StepConfig, functionPath: string): Trigger[] {
  const registeredTriggers: Trigger[] = []

  config.triggers.forEach((triggerConfig) => {
    const engineConfig = triggerConfigToEngineConfig(triggerConfig)

    const trigger = bridge.registerTrigger({
      trigger_type: triggerConfig.type,
      function_path: functionPath,
      config: engineConfig,
    })

    registeredTriggers.push(trigger)
  })

  return registeredTriggers
}

export async function evaluateConditions<TInput>(
  conditions: TriggerCondition<TInput>[] | undefined,
  input: TInput,
  ctx: any,
  triggerInfo: TriggerInfo
): Promise<boolean> {
  if (!conditions || conditions.length === 0) {
    return true
  }

  for (const condition of conditions) {
    const result = await condition(input, ctx, triggerInfo)
    if (!result) {
      return false
    }
  }

  return true
}

export function createConditionWrapper<TInput, TOutput>(
  handler: (input: TInput, ...args: any[]) => Promise<TOutput>,
  conditions: TriggerCondition<TInput>[] | undefined,
  triggerInfo: TriggerInfo
): (input: TInput, ...args: any[]) => Promise<TOutput | undefined> {
  return async (input: TInput, ...args: any[]): Promise<TOutput | undefined> => {
    const ctx = args[0]
    
    if (conditions && conditions.length > 0) {
      const passed = await evaluateConditions(conditions, input, ctx, triggerInfo)
      if (!passed) {
        return undefined
      }
    }

    return handler(input, ...args)
  }
}

function triggerConfigToEngineConfig(trigger: TriggerConfig): Record<string, string> {
  switch (trigger.type) {
    case 'event':
      return {
        topic: trigger.subscribes[0] || '',
      }
    case 'api':
      return {
        api_path: trigger.path,
        http_method: trigger.method,
      }
    case 'cron':
      return {
        expression: trigger.expression,
      }
  }
}
