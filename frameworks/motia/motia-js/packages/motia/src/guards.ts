import type { Step, TriggerConfig } from './types'

// Type aliases to avoid deep type instantiation
type ApiTriggerType = Extract<TriggerConfig, { type: 'http' }>
type QueueTriggerType = Extract<TriggerConfig, { type: 'queue' }>
type CronTriggerType = Extract<TriggerConfig, { type: 'cron' }>
type StateTriggerType = Extract<TriggerConfig, { type: 'state' }>
type StreamTriggerType = Extract<TriggerConfig, { type: 'stream' }>

export const isApiTrigger = (trigger: TriggerConfig): trigger is ApiTriggerType => trigger.type === 'http'

export const isQueueTrigger = (trigger: TriggerConfig): trigger is QueueTriggerType => trigger.type === 'queue'

export const isCronTrigger = (trigger: TriggerConfig): trigger is CronTriggerType => trigger.type === 'cron'

export const isStateTrigger = (trigger: TriggerConfig): trigger is StateTriggerType => trigger.type === 'state'

export const isStreamTrigger = (trigger: TriggerConfig): trigger is StreamTriggerType => trigger.type === 'stream'

export const getApiTriggers = (step: Step): ApiTriggerType[] => step.config.triggers.filter(isApiTrigger)

export const getQueueTriggers = (step: Step): QueueTriggerType[] => step.config.triggers.filter(isQueueTrigger)

export const getCronTriggers = (step: Step): CronTriggerType[] => step.config.triggers.filter(isCronTrigger)

export const getStateTriggers = (step: Step): StateTriggerType[] => step.config.triggers.filter(isStateTrigger)

export const getStreamTriggers = (step: Step): StreamTriggerType[] => step.config.triggers.filter(isStreamTrigger)
