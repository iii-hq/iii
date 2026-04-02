import { type ApiRequest, type ApiResponse, Logger } from 'iii-sdk'
import { iii } from './iii'

// biome-ignore lint/suspicious/noExplicitAny: generic default requires any for handler flexibility
export const useApi = <TBody = any>(
  config: {
    api_path: string
    http_method: string
    description?: string
    metadata?: Record<string, unknown>
  },
  handler: (req: ApiRequest<TBody>, logger: Logger) => Promise<ApiResponse>,
) => {
  const function_id = `api::${config.http_method.toLowerCase()}::${config.api_path}`
  const logger = new Logger(undefined, function_id)

  iii.registerFunction({ id: function_id, metadata: config.metadata }, req => handler(req, logger))
  iii.registerTrigger({
    type: 'http',
    function_id,
    config: {
      api_path: config.api_path,
      http_method: config.http_method,
      description: config.description,
      metadata: config.metadata,
    },
  })
}
