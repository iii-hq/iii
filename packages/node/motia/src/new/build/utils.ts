import {
  type ApiRequest as IIIApiRequest,
  type ApiResponse as IIIApiResponse,
  StreamAuthInput,
  StreamJoinLeaveEvent,
  getContext,
} from '@iii-dev/sdk'
import { isApiStep, isCronStep, isEventStep } from '../../guards'
import { Printer } from '../../printer'
import type {
  ApiMiddleware,
  ApiRouteHandler,
  CronHandler,
  Emitter,
  ApiRequest as MotiaApiRequest,
  ApiResponse as MotiaApiResponse,
} from '../../types'
import { FlowContext, Step, StepConfig, StepHandler } from '../../types'
import { AuthenticateStream, StreamAuthInput as MotiaStreamAuthInput, StreamConfig } from '../../types-stream'
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

const flowContext = <EmitData>(streamManager: Motia): FlowContext<EmitData> => {
  const traceId = crypto.randomUUID()
  const { logger } = getContext()
  const emit: Emitter<EmitData> = async (event: EmitData): Promise<void> => bridge.invokeFunction('emit', event)
  const state = new StateManager()

  return { emit, traceId, state, logger, streams: streamManager.streams }
}

export class Motia {
  public streams: Record<string, Stream<any>> = {}
  private authenticateStream: AuthenticateStream | undefined

  public addStep(config: StepConfig, stepPath: string, handler: StepHandler<any>) {
    const step: StepWithHandler = { config, handler, filePath: stepPath, version: '' }
    const functionPath = `steps.${step.config.name}`

    printer.printStepCreated(step)

    if (isApiStep(step)) {
      bridge.registerFunction({ functionPath }, async (req: IIIApiRequest<any>): Promise<IIIApiResponse> => {
        const context = flowContext(this)

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
        return (step.handler as CronHandler<any>)(flowContext(this))
      })
    } else {
      bridge.registerFunction({ functionPath }, async (req) => {
        return step.handler(req, flowContext(this))
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

  public addStream(config: StreamConfig, streamPath: string) {
    printer.printStreamCreated({ filePath: streamPath, config, hidden: false })
    this.streams[config.name] = new Stream<any>(config)
  }

  public initialize() {
    const hasJoin = Object.values(this.streams).some((stream) => stream.config.onJoin || stream.config.canAccess)
    const hasLeave = Object.values(this.streams).some((stream) => stream.config.onLeave)

    if (this.authenticateStream) {
      const functionPath = 'motia.streams.authenticate'

      bridge.registerFunction({ functionPath }, async (req: StreamAuthInput) => {
        if (this.authenticateStream) {
          const context = flowContext<any>(this)
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
      const functionPath = 'motia.streams.join'

      bridge.registerFunction({ functionPath }, async (req: StreamJoinLeaveEvent) => {
        const { stream_name, group_id, id, context: authContext } = req
        const stream = this.streams[stream_name]
        const context = flowContext<any>(this)

        if (stream?.config.onJoin) {
          return stream.config.onJoin({ groupId: group_id, id }, context, authContext)
        }

        if (stream?.config.canAccess) {
          const canAccess = await stream.config.canAccess({ groupId: group_id, id }, authContext)
          return { unauthorized: !canAccess }
        }
      })

      bridge.registerTrigger({ triggerType: 'streams:join', functionPath, config: {} })
    }

    if (hasLeave) {
      const functionPath = 'motia.streams.leave'

      bridge.registerFunction({ functionPath }, async (req: StreamJoinLeaveEvent) => {
        const { stream_name, group_id, id, context: authContext } = req
        const stream = this.streams[stream_name]
        const context = flowContext<any>(this)

        if (stream?.config.onLeave) {
          await stream.config.onLeave({ groupId: group_id, id }, context, authContext)
        }
      })

      bridge.registerTrigger({ triggerType: 'streams:leave', functionPath, config: {} })
    }
  }
}
