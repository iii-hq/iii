import { type HttpRequest, type HttpResponse } from '@iii-dev/helpers/http'
import { Logger } from 'iii-sdk'
import { iii } from './iii'

// biome-ignore lint/suspicious/noExplicitAny: generic default requires any for handler flexibility
export const useApi = <TBody = any>(
  config: {
    api_path: string
    http_method: string
    description?: string
    metadata?: Record<string, unknown>
  },
  handler: (req: HttpRequest<TBody>, logger: Logger) => Promise<HttpResponse>,
) => {
  const function_id = `api::${config.http_method.toLowerCase()}::${config.api_path}`
  const logger = new Logger(undefined, function_id)

  iii.registerFunction(function_id, req => handler(req, logger), {
    description: config.description,
    metadata: config.metadata,
  })
  // A route is an observer of the http worker's source: its args are the method and path.
  iii.observe(
    {
      function_id: 'http::on-request',
      args: {
        method: config.http_method,
        path: config.api_path.startsWith('/') ? config.api_path : `/${config.api_path}`,
      },
    },
    function_id,
    { metadata: config.metadata },
  )
}
