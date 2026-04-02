import type {
  ExtractApiInput,
  ExtractQueueInput,
  ExtractStateInput,
  ExtractStreamInput,
  FlowContext,
  Handlers,
  StepConfig,
} from './types'

type StepDefinition<TConfig extends StepConfig> = {
  config: TConfig
  handler: Handlers<TConfig>
}

type InferHandlerInput<TConfig extends StepConfig> = TConfig extends StepConfig
  ? Parameters<Handlers<TConfig>>[0]
  : never

type TriggerHandlers<TConfig extends StepConfig> = {
  queue?: (
    input: ExtractQueueInput<InferHandlerInput<TConfig>>,
    ctx: Omit<FlowContext<any, any>, 'match'>,
  ) => Promise<void>
  http?: (
    request: ExtractApiInput<InferHandlerInput<TConfig>>,
    ctx: Omit<FlowContext<any, any>, 'match'>,
  ) => Promise<any>
  cron?: (ctx: Omit<FlowContext<any, any>, 'match'>) => Promise<void>
  state?: (
    input: ExtractStateInput<InferHandlerInput<TConfig>>,
    ctx: Omit<FlowContext<any, any>, 'match'>,
  ) => Promise<any>
  stream?: (
    input: ExtractStreamInput<InferHandlerInput<TConfig>>,
    ctx: Omit<FlowContext<any, any>, 'match'>,
  ) => Promise<any>
}

type MultiTriggerStepBuilder<TConfig extends StepConfig> = {
  config: TConfig
  handlers: (handlers: TriggerHandlers<TConfig>) => StepDefinition<TConfig>

  // Chainable methods
  onQueue: (
    handler: TriggerHandlers<TConfig>['queue'],
  ) => MultiTriggerStepBuilder<TConfig> & { handlers: () => StepDefinition<TConfig> }
  onHttp: (
    handler: TriggerHandlers<TConfig>['http'],
  ) => MultiTriggerStepBuilder<TConfig> & { handlers: () => StepDefinition<TConfig> }
  onCron: (
    handler: TriggerHandlers<TConfig>['cron'],
  ) => MultiTriggerStepBuilder<TConfig> & { handlers: () => StepDefinition<TConfig> }
  onState: (
    handler: TriggerHandlers<TConfig>['state'],
  ) => MultiTriggerStepBuilder<TConfig> & { handlers: () => StepDefinition<TConfig> }
  onStream: (
    handler: TriggerHandlers<TConfig>['stream'],
  ) => MultiTriggerStepBuilder<TConfig> & { handlers: () => StepDefinition<TConfig> }
}

export function multiTriggerStep<const TConfig extends StepConfig>(config: TConfig): MultiTriggerStepBuilder<TConfig> {
  const collectedHandlers: Partial<TriggerHandlers<TConfig>> = {}

  const createUnifiedHandler = (): Handlers<TConfig> => {
    return (async (input: any, ctx: any) => {
      if (ctx.trigger.type === 'queue' && collectedHandlers.queue) {
        return collectedHandlers.queue(input, ctx)
      }
      if (ctx.trigger.type === 'http' && collectedHandlers.http) {
        return collectedHandlers.http(input, ctx)
      }
      if (ctx.trigger.type === 'cron' && collectedHandlers.cron) {
        return collectedHandlers.cron(ctx)
      }
      if (ctx.trigger.type === 'state' && collectedHandlers.state) {
        return collectedHandlers.state(input, ctx)
      }
      if (ctx.trigger.type === 'stream' && collectedHandlers.stream) {
        return collectedHandlers.stream(input, ctx)
      }

      ctx.logger.warn(`No handler defined for trigger type: ${ctx.trigger.type}`, {
        availableHandlers: Object.keys(collectedHandlers),
        triggerType: ctx.trigger.type,
      })

      throw new Error(
        `No handler defined for trigger type: ${ctx.trigger.type}. Available handlers: ${Object.keys(collectedHandlers).join(', ')}`,
      )
    }) as Handlers<TConfig>
  }

  const builder: any = {
    config,

    onQueue(handler: TriggerHandlers<TConfig>['queue']) {
      collectedHandlers.queue = handler
      return {
        ...builder,
        handlers: () => ({ config, handler: createUnifiedHandler() }),
      }
    },

    onHttp(handler: TriggerHandlers<TConfig>['http']) {
      collectedHandlers.http = handler
      return {
        ...builder,
        handlers: () => ({ config, handler: createUnifiedHandler() }),
      }
    },

    onCron(handler: TriggerHandlers<TConfig>['cron']) {
      collectedHandlers.cron = handler
      return {
        ...builder,
        handlers: () => ({ config, handler: createUnifiedHandler() }),
      }
    },

    onState(handler: TriggerHandlers<TConfig>['state']) {
      collectedHandlers.state = handler
      return {
        ...builder,
        handlers: () => ({ config, handler: createUnifiedHandler() }),
      }
    },

    onStream(handler: TriggerHandlers<TConfig>['stream']) {
      collectedHandlers.stream = handler
      return {
        ...builder,
        handlers: () => ({ config, handler: createUnifiedHandler() }),
      }
    },

    handlers(h?: TriggerHandlers<TConfig>) {
      if (h) {
        Object.assign(collectedHandlers, h)
      }
      return { config, handler: createUnifiedHandler() }
    },
  }

  return builder
}
