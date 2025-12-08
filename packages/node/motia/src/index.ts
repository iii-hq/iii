import { type ApiRequest as IIIApiRequest, type ApiResponse as IIIApiResponse, getContext } from '@iii-dev/sdk'
import { bridge } from './new/bridge'
import { Stream } from './new/streams'
import { isApiStep, isCronStep, isEventStep } from './guards'
import { FlowContext, Step, StepConfig, StepHandler } from './types'
import { StreamConfig } from './types-stream'
import { existsSync } from 'fs'
import { globSync } from 'glob'
import { Printer } from './printer'
import type { ApiRouteHandler, ApiRequest as MotiaApiRequest, ApiResponse as MotiaApiResponse } from './types'

export * from './types'
export * from './types-stream'
export { Stream } from './new/streams'

const printer = new Printer(process.cwd())

export const getStreamFilesFromDir = (dir: string): string[] => {
  if (!existsSync(dir)) {
    return []
  }
  return globSync('**/*.stream.{ts,js}', { absolute: true, cwd: dir })
}

export const getStepFilesFromDir = (dir: string): string[] => {
  if (!existsSync(dir)) {
    return []
  }
  return globSync('**/*.step.{ts,js}', { absolute: true, cwd: dir })
}

type StepWithHandler = Step & { handler: StepHandler<any> }

export const stepWrapper = (
  config: StepConfig,
  stepPath: string,
  handler: StepHandler<any>,
  streams: Record<string, Stream<any>>,
) => {
  const step: StepWithHandler = { config, handler, filePath: stepPath, version: '' }
  const functionPath = `steps.${step.config.name}`

  printer.printStepCreated(step)

  if (isApiStep(step)) {
    bridge.registerFunction({ functionPath }, async (req: IIIApiRequest<any>): Promise<IIIApiResponse> => {
      const { logger } = getContext()
      const emit = <TData>(event: TData): Promise<void> => bridge.invokeFunction('emit', { event })
      const context: FlowContext<any> = {
        emit,
        traceId: crypto.randomUUID(),
        state: null as never,
        logger,
        streams,
      }

      const motiaReq: MotiaApiRequest<any> = {
        pathParams: req.path_params,
        queryParams: req.query_params,
        body: req.body,
        headers: req.headers,
      }

      const handler = step.handler as ApiRouteHandler<any, MotiaApiResponse, any>
      const response: MotiaApiResponse = await handler(motiaReq, context)

      return {
        status_code: response.status,
        headers: response.headers,
        body: response.body,
      }
    })
  } else {
    bridge.registerFunction({ functionPath }, async (req) => {
      const { logger } = getContext()
      const emit = <TData>(event: TData): Promise<void> => bridge.invokeFunction('emit', { event })
      const context: FlowContext<any> = {
        emit,
        traceId: crypto.randomUUID(),
        state: null as never,
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
    bridge.registerTrigger({
      triggerType: 'event',
      functionPath,
      config: { topic: step.config.subscribes[0] }, // TODO we need to ensure that we support multiple topics
    })
  } else if (isCronStep(step)) {
    bridge.registerTrigger({
      triggerType: 'cron',
      functionPath,
      config: { cron: step.config.cron },
    })
  }
}

export const streamWrapper = (config: StreamConfig, streamPath: string) => {
  printer.printStreamCreated({ filePath: streamPath, config, hidden: false })

  return new Stream<any>(config.name)
}
