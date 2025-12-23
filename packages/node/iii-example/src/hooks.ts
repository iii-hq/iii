import { bridge } from './bridge'
import { type ApiResponse, type ApiRequest, type FunctionMessage, getContext, Context } from '@iii-dev/sdk'

export const useApi = (
  config: { name: string; api_path: string; http_method: string; description: string; metadata: Record<string, any> },
  handler: (req: ApiRequest<any>, context: Context) => Promise<ApiResponse>,
) => {
  bridge.registerFunction({ functionPath: config.name, metadata: config.metadata }, (req) => handler(req, getContext()))
  bridge.registerTrigger({
    triggerType: 'api',
    functionPath: config.name,
    config: { api_path: config.api_path, http_method: config.http_method, description: config.description, metadata: config.metadata },
  })
}

/**
 * Subscribe to receive notifications when the list of available functions changes.
 * @param callback - Function to be called with the list of available functions
 * @returns A function to unsubscribe from the notifications
 */
export const useFunctionsAvailable = (callback: (functions: FunctionMessage[]) => void): (() => void) => {
  return bridge.onFunctionsAvailable(callback)
}
