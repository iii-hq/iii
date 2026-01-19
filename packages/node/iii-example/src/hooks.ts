import { type ApiRequest, type ApiResponse, type Context, type FunctionMessage, getContext } from '@iii-dev/sdk'
import { bridge } from './bridge'

export const useApi = (
  config: { api_path: string; http_method: string; description?: string; metadata?: Record<string, any> },
  handler: (req: ApiRequest<any>, context: Context) => Promise<ApiResponse>,
) => {
  const function_path = `api.${config.http_method.toLowerCase()}.${config.api_path}`

  bridge.registerFunction({ function_path, metadata: config.metadata }, (req) => handler(req, getContext()))
  bridge.registerTrigger({
    trigger_type: 'api',
    function_path,
    config: {
      api_path: config.api_path,
      http_method: config.http_method,
      description: config.description,
      metadata: config.metadata,
    },
  })
}

export const useFunctionsAvailable = (callback: (functions: FunctionMessage[]) => void): (() => void) => {
  return bridge.onFunctionsAvailable(callback)
}

export type LogEvent = {
  id: string
  data: Record<string, unknown>
  function_name: string
  message: string
  time: number
  trace_id: string
  level: 'info' | 'warn' | 'error'
}

export type LogLevel = 'info' | 'warn' | 'error' | 'all'
type LogTriggerConfig = { level: LogLevel; description?: string }
type LogTriggerHandler = (log: LogEvent, context: Context) => Promise<void>

export const useOnLog = (config: LogTriggerConfig, handler: LogTriggerHandler) => {
  const function_path = `onLog.${config.level}.${Date.now()}`

  bridge.registerFunction({ function_path, description: config.description, metadata: {} }, (log) =>
    handler(log, getContext()),
  )
  bridge.registerTrigger({
    trigger_type: 'log',
    function_path,
    config: {
      level: config.level,
      description: config.description,
    },
  })
}
