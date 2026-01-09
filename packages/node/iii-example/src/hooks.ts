import { type ApiRequest, type ApiResponse, type Context, type FunctionMessage, getContext } from '@iii-dev/sdk'
import { bridge } from './bridge'

export const useApi = (
  config: { api_path: string; http_method: string; description?: string; metadata: Record<string, any> },
  handler: (req: ApiRequest<any>, context?: Context) => Promise<ApiResponse>,
) => {
  const function_path = `api.${config.http_method.toLowerCase()}.${config.api_path}`

  bridge.registerFunction({ function_path, metadata: config.metadata }, (req) => handler(req))
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

export const useCron = (
  config: { expression: string; description?: string },
  handler: (event: { job_id: string; scheduled_time: string; actual_time: string }, context: Context) => Promise<any>,
) => {
  const function_path = `cron.${config.expression.replace(/\s+/g, '_').replace(/\*/g, 'x')}`

  bridge.registerFunction({ function_path, metadata: { type: 'cron', description: config.description } }, (event) =>
    handler(event, getContext()),
  )
  bridge.registerTrigger({
    trigger_type: 'cron',
    function_path,
    config: {
      expression: config.expression,
      description: config.description,
    },
  })
}

export const useEvent = (
  config: { topic: string; description?: string },
  handler: (event: { topic: string; data: any; timestamp: string }, context: Context) => Promise<any>,
) => {
  const function_path = `event.${config.topic.replace(/\./g, '_')}`

  bridge.registerFunction({ function_path, metadata: { type: 'event', topic: config.topic, description: config.description } }, (event) =>
    handler(event, getContext()),
  )
  bridge.registerTrigger({
    trigger_type: 'event',
    function_path,
    config: {
      topic: config.topic,
      description: config.description,
    },
  })
}

export const useOnJoin = (
  config: { description?: string },
  handler: (event: { stream_name: string; user_id: string; timestamp: string }, context: Context) => Promise<any>,
) => {
  const function_path = `streams.on_join.${Date.now()}`

  bridge.registerFunction({ function_path, metadata: { type: 'stream_join', description: config.description } }, (event) =>
    handler(event, getContext()),
  )
  bridge.registerTrigger({
    trigger_type: 'streams:join',
    function_path,
    config: {
      description: config.description,
    },
  })
}

export const useOnLeave = (
  config: { description?: string },
  handler: (event: { stream_name: string; user_id: string; timestamp: string }, context: Context) => Promise<any>,
) => {
  const function_path = `streams.on_leave.${Date.now()}`

  bridge.registerFunction({ function_path, metadata: { type: 'stream_leave', description: config.description } }, (event) =>
    handler(event, getContext()),
  )
  bridge.registerTrigger({
    trigger_type: 'streams:leave',
    function_path,
    config: {
      description: config.description,
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

export const useOnLog = (
  config: LogTriggerConfig,
  handler: LogTriggerHandler,
) => {
  const function_path = `onLog.${config.level}.${Date.now()}`

  bridge.registerFunction({ function_path, description: config.description, metadata: {} }, (log) => handler(log, getContext()))
  bridge.registerTrigger({
    trigger_type: 'log',
    function_path,
    config: {
      level: config.level,
      description: config.description,
    },
  })
}
