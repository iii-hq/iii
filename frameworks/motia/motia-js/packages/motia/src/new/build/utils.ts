import type {
  ApiRequest as IIIApiRequest,
  ApiResponse as IIIApiResponse,
  HttpRequest as IIIHttpRequest,
  HttpResponse as IIIHttpResponse,
} from 'iii-sdk'
import { http as iiiHttp, Logger } from 'iii-sdk'
import type { StreamAuthInput, StreamJoinLeaveEvent } from 'iii-sdk/stream'
import { currentTraceId } from 'iii-sdk/telemetry'
import { isApiTrigger, isCronTrigger, isQueueTrigger, isStateTrigger, isStreamTrigger } from '../../guards'
import type {
  ApiMiddleware,
  ApiResponse,
  ExtractApiInput,
  ExtractDataPayload,
  ExtractQueueInput,
  ExtractStateInput,
  ExtractStreamInput,
  FlowContext,
  MatchHandlers,
  ApiRequest as MotiaApiRequest,
  MotiaHttpArgs,
  QueueConfig,
  Step,
  StepConfig,
  StepHandler,
  TriggerConfig,
  TriggerInfo,
} from '../../types'
import type { AuthenticateStream, StreamAuthInput as MotiaStreamAuthInput, StreamConfig } from '../../types-stream'
import { getInstance } from '../iii'
import { setupStepEndpoint } from '../setup-step-endpoint'
import { Stream } from '../stream'
import { logger } from '../logger'

type StepWithHandler = Step & { handler: StepHandler<unknown> }

type TriggerConfigBase = {
  metadata: StepConfig & { filePath: string }
  condition_function_id?: string
}

type ApiTriggerConfig = TriggerConfigBase & {
  api_path: string
  http_method: string
}

type QueueTriggerConfig = TriggerConfigBase & {
  topic: string
  queue_config?: Partial<QueueConfig>
}

type CronTriggerConfig = TriggerConfigBase & {
  expression: string
}

const composeMiddleware = <TRequestBody = unknown, TEnqueueData = never>(
  ...middlewares: ApiMiddleware<TRequestBody, TEnqueueData>[]
) => {
  return async (
    args: MotiaHttpArgs<TRequestBody>,
    ctx: FlowContext<TEnqueueData, MotiaHttpArgs<TRequestBody>>,
    handler: () => Promise<ApiResponse | void>,
  ): Promise<ApiResponse | void> => {
    const composedHandler = middlewares.reduceRight<() => Promise<ApiResponse | void>>(
      (nextHandler, middleware) => () => middleware(args, ctx, nextHandler),
      handler,
    )

    return composedHandler()
  }
}

const flowContext = <EnqueueData, TInput = unknown>(
  trigger: TriggerInfo,
  input?: TInput,
): FlowContext<EnqueueData, TInput> => {
  const traceId = currentTraceId() ?? crypto.randomUUID()
  const context: FlowContext<EnqueueData, TInput> = {
    traceId,
    trigger,

    is: {
      queue: (inp: TInput): inp is ExtractQueueInput<TInput> => trigger.type === 'queue',
      http: (inp: TInput): inp is ExtractApiInput<TInput> => trigger.type === 'http',
      cron: (inp: TInput): inp is never => trigger.type === 'cron',
      state: (inp: TInput): inp is ExtractStateInput<TInput> => trigger.type === 'state',
      stream: (inp: TInput): inp is ExtractStreamInput<TInput> => trigger.type === 'stream',
    },

    getData: (): ExtractDataPayload<TInput> => {
      if (trigger.type === 'http') {
        return (input as Extract<TInput, MotiaHttpArgs>).request.body as ExtractDataPayload<TInput>
      }
      return input as ExtractDataPayload<TInput>
    },

    match: async <TResult = unknown>(
      handlers: MatchHandlers<TInput, EnqueueData, TResult>,
    ): Promise<TResult | undefined> => {
      if (trigger.type === 'queue' && handlers.queue) {
        await handlers.queue(input as ExtractQueueInput<TInput>)
        return undefined
      }
      if (trigger.type === 'http' && handlers.http) {
        return await handlers.http(input as ExtractApiInput<TInput>)
      }
      if (trigger.type === 'cron' && handlers.cron) {
        await handlers.cron()
        return undefined
      }
      if (trigger.type === 'state' && handlers.state) {
        return await handlers.state(input as ExtractStateInput<TInput>)
      }
      if (trigger.type === 'stream' && handlers.stream) {
        return await handlers.stream(input as ExtractStreamInput<TInput>)
      }
      if (handlers.default) {
        return await handlers.default(input as TInput)
      }

      logger.warn(`No handler matched for trigger type: ${trigger.type}`, {
        availableHandlers: Object.keys(handlers).filter((k) => k !== 'default'),
        triggerType: trigger.type,
      })

      throw new Error(
        `No handler matched for trigger type: ${trigger.type}. Available handlers: ${Object.keys(handlers).join(', ')}`,
      )
    },
  }

  return context
}

function getTriggerSuffix(trigger: TriggerConfig): string {
  if (isApiTrigger(trigger)) return `http(${trigger.method} ${trigger.path})`
  if (isCronTrigger(trigger)) return `cron(${trigger.expression})`
  if (isQueueTrigger(trigger)) return `queue(${trigger.topic})`
  if (isStreamTrigger(trigger)) return `stream(${trigger.streamName})`
  if (isStateTrigger(trigger)) return 'state'
  return 'unknown'
}

export class Motia {
  public streams: Record<string, Stream> = {}
  private authenticateStream: AuthenticateStream | undefined

  public addStep(config: StepConfig, stepPath: string, handler: StepHandler<unknown>, filePath: string) {
    const step: StepWithHandler = { config, handler, filePath: stepPath }
    const metadata = { ...step.config, filePath }

    const seenSuffixes = new Set<string>()

    step.config.triggers.forEach((trigger: TriggerConfig, index: number) => {
      let triggerSuffix = getTriggerSuffix(trigger)
      if (seenSuffixes.has(triggerSuffix)) {
        triggerSuffix = `${triggerSuffix}-${index}`
      }
      seenSuffixes.add(triggerSuffix)
      const function_id = `steps::${step.config.name}::trigger::${triggerSuffix}`

      if (isApiTrigger(trigger)) {
        getInstance().registerFunction(
          { id: function_id, metadata },
          // biome-ignore lint/suspicious/noConfusingVoidType: void is necessary here
          iiiHttp(async (req: IIIHttpRequest, res: IIIHttpResponse): Promise<void | IIIApiResponse> => {
            const triggerInfo: TriggerInfo = { type: 'http', index }
            const motiaRequest: MotiaHttpArgs<unknown> = {
              request: {
                pathParams: req.path_params || {},
                queryParams: req.query_params || {},
                body: req.body,
                headers: req.headers || {},
                method: req.method,
                requestBody: req.request_body,
              },
              response: res,
            }
            const context = flowContext(triggerInfo, motiaRequest)
            const middlewares = Array.isArray(trigger?.middleware) ? trigger.middleware : []

            if (middlewares.length > 0) {
              const composed = composeMiddleware(...middlewares)
              const result = await composed(motiaRequest, context, () =>
                step.handler(motiaRequest, context as FlowContext),
              )

              if (result) {
                return {
                  status_code: result.status,
                  body: result.body,
                  headers: result.headers,
                }
              }
            } else {
              const result = await step.handler(motiaRequest, context as FlowContext)

              if (result) {
                return {
                  status_code: result.status,
                  body: result.body,
                  headers: result.headers,
                }
              }
            }
          }),
        )

        const apiPath = trigger.path.startsWith('/') ? trigger.path.substring(1) : trigger.path
        const triggerConfig: ApiTriggerConfig = {
          api_path: apiPath,
          http_method: trigger.method,
          metadata,
        }

        if (trigger.condition) {
          const conditionPath = `${function_id}::conditions::${index}`

          getInstance().registerFunction(
            { id: conditionPath },
            async (req: IIIApiRequest<unknown>): Promise<unknown> => {
              const triggerInfo: TriggerInfo = { type: 'http', index }
              const motiaRequest: MotiaApiRequest<unknown> = {
                pathParams: req.path_params || {},
                queryParams: req.query_params || {},
                body: req.body,
                headers: req.headers || {},
              }

              return trigger.condition?.(motiaRequest, flowContext(triggerInfo, motiaRequest))
            },
          )

          triggerConfig.condition_function_id = conditionPath
        }

        getInstance().registerTrigger({
          type: 'http',
          function_id,
          config: triggerConfig,
        })
      } else if (isQueueTrigger(trigger)) {
        getInstance().registerFunction({ id: function_id, metadata }, async (req) => {
          const triggerInfo: TriggerInfo = { type: 'queue', index }
          const context = flowContext(triggerInfo, req)
          return step.handler(req, context)
        })

        const triggerConfig: QueueTriggerConfig = {
          topic: trigger.topic,
          metadata: { ...metadata },
          ...(trigger.config ? { queue_config: trigger.config } : {}),
        }

        if (trigger.condition) {
          const conditionPath = `${function_id}::conditions::${index}`

          getInstance().registerFunction({ id: conditionPath }, async (input: unknown) => {
            const triggerInfo: TriggerInfo = { type: 'queue', index }

            return trigger.condition?.(input, flowContext(triggerInfo, input))
          })

          triggerConfig.condition_function_id = conditionPath
        }

        getInstance().registerTrigger({
          type: 'queue',
          function_id,
          config: triggerConfig,
        })
      } else if (isCronTrigger(trigger)) {
        getInstance().registerFunction({ id: function_id, metadata }, async (_req): Promise<unknown> => {
          const triggerInfo: TriggerInfo = { type: 'cron', index }
          return step.handler(undefined, flowContext(triggerInfo))
        })

        const triggerConfig: CronTriggerConfig = {
          expression: trigger.expression,
          metadata,
        }

        if (trigger.condition) {
          const conditionPath = `${function_id}::conditions::${index}`

          getInstance().registerFunction({ id: conditionPath }, async () => {
            const triggerInfo: TriggerInfo = { type: 'cron', index }
            return trigger.condition?.(undefined, flowContext(triggerInfo))
          })

          triggerConfig.condition_function_id = conditionPath
        }

        getInstance().registerTrigger({
          type: 'cron',
          function_id,
          config: triggerConfig,
        })
      } else if (isStateTrigger(trigger)) {
        getInstance().registerFunction({ id: function_id, metadata }, async (req) => {
          const triggerInfo: TriggerInfo = { type: 'state', index }
          const context = flowContext(triggerInfo, req)
          return step.handler(req, context)
        })

        // biome-ignore lint/suspicious/noExplicitAny: needed for trigger config
        const triggerConfig: Record<string, any> = { metadata }

        if (trigger.condition) {
          const conditionPath = `${function_id}::conditions::${index}`

          getInstance().registerFunction({ id: conditionPath }, async (input) => {
            const triggerInfo: TriggerInfo = { type: 'state', index }
            return trigger.condition?.(input, flowContext(triggerInfo, input))
          })

          triggerConfig.condition_function_id = conditionPath
        }

        getInstance().registerTrigger({
          type: 'state',
          function_id,
          config: triggerConfig,
        })
      } else if (isStreamTrigger(trigger)) {
        getInstance().registerFunction({ id: function_id, metadata }, async (req) => {
          const triggerInfo: TriggerInfo = { type: 'stream', index }
          const context = flowContext(triggerInfo, req)
          return step.handler(req, context)
        })

        type StreamTriggerConfig = {
          metadata: StepConfig
          stream_name: string
          group_id?: string
          item_id?: string
          condition_function_id?: string
        }

        const triggerConfig: StreamTriggerConfig = {
          metadata,
          stream_name: trigger.streamName,
          group_id: trigger.groupId,
          item_id: trigger.itemId,
        }

        if (trigger.condition) {
          const conditionPath = `${function_id}::conditions::${index}`

          getInstance().registerFunction({ id: conditionPath }, async (input) => {
            const triggerInfo: TriggerInfo = { type: 'stream', index }
            return trigger.condition?.(input, flowContext(triggerInfo, input))
          })

          triggerConfig.condition_function_id = conditionPath
        }

        getInstance().registerTrigger({
          type: 'stream',
          function_id,
          config: triggerConfig,
        })
      }
    })
  }

  public addStream(config: StreamConfig, _streamPath: string) {
    this.streams[config.name] = new Stream(config)
  }

  public initialize() {
    const hasJoin = Object.values(this.streams).some((stream) => stream.config.onJoin)
    const hasLeave = Object.values(this.streams).some((stream) => stream.config.onLeave)

    setupStepEndpoint(getInstance())

    if (this.authenticateStream) {
      const function_id = 'motia::stream::authenticate'

      getInstance().registerFunction({ id: function_id }, async (req: StreamAuthInput) => {
        if (this.authenticateStream) {
          const triggerInfo: TriggerInfo = { type: 'queue' }
          const context = flowContext(triggerInfo)
          const input: MotiaStreamAuthInput = {
            headers: req.headers,
            path: req.path,
            queryParams: req.query_params,
            addr: req.addr,
          }

          return this.authenticateStream(input, context)
        }
      })
    }

    if (hasJoin) {
      const function_id = 'motia::stream::join'

      getInstance().registerFunction({ id: function_id }, async (req: StreamJoinLeaveEvent) => {
        const { stream_name, group_id, id, context: authContext } = req
        const stream = this.streams[stream_name]
        const triggerInfo: TriggerInfo = { type: 'queue' }
        const context = flowContext(triggerInfo)

        if (stream?.config.onJoin) {
          return stream.config.onJoin({ groupId: group_id, id }, context, authContext)
        }
      })

      getInstance().registerTrigger({
        type: 'stream:join',
        function_id,
        config: {},
      })
    }

    if (hasLeave) {
      const function_id = 'motia::stream::leave'

      getInstance().registerFunction({ id: function_id }, async (req: StreamJoinLeaveEvent) => {
        const { stream_name, group_id, id, context: authContext } = req
        const stream = this.streams[stream_name]
        const triggerInfo: TriggerInfo = { type: 'queue' }
        const context = flowContext(triggerInfo)

        if (stream?.config.onLeave) {
          await stream.config.onLeave({ groupId: group_id, id }, context, authContext)
        }
      })

      getInstance().registerTrigger({
        type: 'stream:leave',
        function_id,
        config: {},
      })
    }
  }
}
