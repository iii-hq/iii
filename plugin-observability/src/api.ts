import type { ApiResponse, MotiaPluginContext } from '@motiadev/core'

export const api = ({ tracerFactory, registerApi }: MotiaPluginContext): void => {
  // POST /__motia/trace/clear - Clear all traces
  registerApi(
    {
      method: 'POST',
      path: '/__motia/trace/clear',
    },
    async (): Promise<ApiResponse> => {
      try {
        await tracerFactory.clear()
        return {
          status: 200,
          body: { message: 'Traces cleared' },
        }
      } catch (error: unknown) {
        return {
          status: 500,
          body: { error: error instanceof Error ? error.message : 'Unknown error' },
        }
      }
    },
  )
}
