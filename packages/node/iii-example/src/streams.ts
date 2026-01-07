import { bridge } from './bridge'

export const streams = {
  get: async (stream_name: string, group_id: string, item_id: string): Promise<any | null> => {
    return bridge.invokeFunction('streams.get', { stream_name, group_id, item_id })
  },
  set: async (stream_name: string, group_id: string, item_id: string, data: any): Promise<any> => {
    return bridge.invokeFunction('streams.set', { stream_name, group_id, item_id, data })
  },
  delete: async (stream_name: string, group_id: string, item_id: string): Promise<void> => {
    return bridge.invokeFunction('streams.delete', { stream_name, group_id, item_id })
  },
  getGroup: async (stream_name: string, group_id: string): Promise<any[]> => {
    return bridge.invokeFunction('streams.getGroup', { stream_name, group_id })
  },
}

export const emit = async (topic: string, data: any): Promise<void> => {
  return bridge.invokeFunction('emit', { topic, data })
}

export const logger = {
  info: (message: string, context?: Record<string, any>): void => {
    bridge.invokeFunctionAsync('logger.info', { message, function_name: 'todo-app', data: context || {}, trace_id: 'app' })
  },
  warn: (message: string, context?: Record<string, any>): void => {
    bridge.invokeFunctionAsync('logger.warn', { message, function_name: 'todo-app', data: context || {}, trace_id: 'app' })
  },
  error: (message: string, context?: Record<string, any>): void => {
    bridge.invokeFunctionAsync('logger.error', { message, function_name: 'todo-app', data: context || {}, trace_id: 'app' })
  },
  debug: (message: string, context?: Record<string, any>): void => {
    bridge.invokeFunctionAsync('logger.debug', { message, function_name: 'todo-app', data: context || {}, trace_id: 'app' })
  },
}
