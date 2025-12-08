import { bridge } from './bridge'
import { type ApiResponse, type ApiRequest, getContext, Context } from '@iii-dev/sdk'

export const useApi = (
  config: { name: string; api_path: string; http_method: string },
  handler: (req: ApiRequest<any>, context: Context) => Promise<ApiResponse>,
) => {
  bridge.registerFunction({ functionPath: config.name }, (req) => handler(req, getContext()))
  bridge.registerTrigger({
    triggerType: 'api',
    functionPath: config.name,
    config: { api_path: config.api_path, http_method: config.http_method },
  })
}
