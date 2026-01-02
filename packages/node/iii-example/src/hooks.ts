import { type ApiRequest, type ApiResponse, type Context, type FunctionMessage, getContext } from '@iii-dev/sdk'
import { bridge } from './bridge'

export const useApi = (
  config: { name: string; api_path: string; http_method: string; description: string; metadata: Record<string, any> },
  handler: (req: ApiRequest<any>, context: Context) => Promise<ApiResponse>,
) => {
  bridge.registerFunction({ function_path: config.name, metadata: config.metadata }, (req) => handler(req, getContext()))
  bridge.registerTrigger({
    trigger_type: 'api',
    function_path: config.name,
    config: { api_path: config.api_path, http_method: config.http_method, description: config.description, metadata: config.metadata },
  })
}

export const useFunctionsAvailable = (callback: (functions: FunctionMessage[]) => void): (() => void) => {
  return bridge.onFunctionsAvailable(callback)
}
