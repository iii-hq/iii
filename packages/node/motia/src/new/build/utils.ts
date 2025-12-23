import { type ApiRequest as IIIApiRequest, type ApiResponse as IIIApiResponse, getContext } from '@iii-dev/sdk'
import { isApiStep, isCronStep, isEventStep } from '../../guards'
import { Printer } from '../../printer'
import type {
  ApiMiddleware,
  ApiRouteHandler,
  CronHandler,
  EmitData,
  Emitter,
  ApiRequest as MotiaApiRequest,
  ApiResponse as MotiaApiResponse,
} from '../../types'
import { FlowContext, Step, StepConfig, StepHandler } from '../../types'
import { StreamConfig } from '../../types-stream'
import { bridge } from '../bridge'
import { StateManager } from '../state'
import { Stream } from '../streams'

const printer = new Printer(process.cwd())

type StepWithHandler = Step & { handler: StepHandler<any> }

const composeMiddleware = <TRequestBody = unknown, TResponseBody = unknown>(
  ...middlewares: ApiMiddleware<TRequestBody, any, TResponseBody>[]
) => {
  return async (req: any, ctx: any, handler: () => Promise<any>): Promise<any> => {
    const composedHandler = middlewares.reduceRight<() => Promise<any>>(
      (nextHandler, middleware) => () => middleware(req, ctx, nextHandler),
      handler,
    )

    return composedHandler()
  }
}

export const stepWrapper = (
  config: StepConfig,
  stepPath: string,
  handler: StepHandler<any>,
  streams: Record<string, Stream<any>>,
): void => {
  const step: StepWithHandler = { config, handler, filePath: stepPath, version: '' }
  const functionPath = `steps.${step.config.name}`
  const state = new StateManager()
  const emit: Emitter<EmitData> = async (event: EmitData): Promise<void> => bridge.invokeFunction('emit', event)

  printer.printStepCreated(step)

  if (isApiStep(step)) {
    bridge.registerFunction({ functionPath }, async (req: IIIApiRequest<any>): Promise<IIIApiResponse> => {
      const { logger } = getContext()
      const context: FlowContext<any> = {
        emit,
        traceId: crypto.randomUUID(),
        state,
        logger,
        streams,
      }

      const motiaReq: MotiaApiRequest<any> = {
        pathParams: req.path_params,
        queryParams: req.query_params,
        body: req.body,
        headers: req.headers,
      }

      const middlewares = Array.isArray(step.config.middleware) ? step.config.middleware : []
      const handler = composeMiddleware(...middlewares)
      const handlerFn = async () => {
        const stepHandler = step.handler as ApiRouteHandler<any, MotiaApiResponse, any>
        return stepHandler(motiaReq, context)
      }
      const response: MotiaApiResponse = await handler(motiaReq, context, handlerFn)

      return {
        status_code: response.status,
        headers: response.headers,
        body: response.body,
      }
    })
  } else if (isCronStep(step)) {
    bridge.registerFunction({ functionPath }, async () => {
      const { logger } = getContext()
      const context: FlowContext<any> = {
        emit,
        traceId: crypto.randomUUID(),
        state,
        logger,
        streams,
      }

      return (step.handler as CronHandler<any>)(context)
    })
  } else {
    bridge.registerFunction({ functionPath }, async (req) => {
      const { logger } = getContext()
      const context: FlowContext<any> = {
        emit,
        traceId: crypto.randomUUID(),
        state,
        logger,
        streams,
      }

      return step.handler(req, context)
    })
  }

  if (isApiStep(step)) {
    const apiPath = step.config.path.startsWith('/') ? step.config.path.substring(1) : step.config.path

    bridge.registerTrigger({
      triggerType: 'api',
      functionPath,
      config: { api_path: apiPath, http_method: step.config.method },
    })
  } else if (isEventStep(step)) {
    step.config.subscribes.forEach((topic) => {
      bridge.registerTrigger({
        triggerType: 'event',
        functionPath,
        config: { topic },
      })
    })
  } else if (isCronStep(step)) {
    bridge.registerTrigger({
      triggerType: 'cron',
      functionPath,
      config: { expression: step.config.cron },
    })
  }
}

export const streamWrapper = (config: StreamConfig, streamPath: string): Stream<any> => {
  printer.printStreamCreated({ filePath: streamPath, config, hidden: false })

  return new Stream<any>(config.name)
}
