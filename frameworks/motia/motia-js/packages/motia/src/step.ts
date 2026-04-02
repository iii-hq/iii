import type { Handlers, StepConfig } from './types'

type StepDefinition<TConfig extends StepConfig> = {
  config: TConfig
  handler: Handlers<TConfig>
}

type StepBuilder<TConfig extends StepConfig> = {
  config: TConfig
  handle: (handler: Handlers<TConfig>) => StepDefinition<TConfig>
}

export function step<TConfig extends StepConfig>(config: TConfig, handler: Handlers<TConfig>): StepDefinition<TConfig>

export function step<TConfig extends StepConfig>(config: TConfig): StepBuilder<TConfig>

export function step<TConfig extends StepConfig>(
  config: TConfig,
  handler?: Handlers<TConfig>,
): StepDefinition<TConfig> | StepBuilder<TConfig> {
  if (handler) {
    return { config, handler }
  }
  return {
    config,
    handle: (h: Handlers<TConfig>) => ({ config, handler: h }),
  }
}
