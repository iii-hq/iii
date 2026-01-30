import { type ApiRequest, type ApiResponse, type Context, type FunctionMessage, getContext, LogCallback, LogConfig } from '@iii-dev/sdk';
import { bridge } from './bridge';

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

export const useOnLog = (handler: LogCallback, config?: LogConfig): (() => void) => {
  return bridge.onLog(handler, config)
}