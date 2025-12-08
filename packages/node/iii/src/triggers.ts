type TriggerConfig<TConfig> = {
  id: string
  functionPath: string
  config: TConfig
}

export type TriggerHandler<TConfig> = {
  registerTrigger(config: TriggerConfig<TConfig>): Promise<void>
  unregisterTrigger(config: TriggerConfig<TConfig>): Promise<void>
}
