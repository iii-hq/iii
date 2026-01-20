import type { ApiRequest, ApiResponse, FlowContext, MotiaPluginContext } from '@motiadev/core'

export const api = (motia: MotiaPluginContext): void => {
  motia.registerApi(
    {
      method: 'GET',
      path: '/__motia/state',
    },
    async (req: ApiRequest): Promise<ApiResponse> => {
      try {
        const groupId = req.queryParams.groupId as string | undefined
        const filter = req.queryParams.filter ? JSON.parse(req.queryParams.filter as string) : undefined
        const items = await motia.state.items({ groupId, filter })

        return {
          status: 200,
          body: items as unknown as Record<string, unknown>,
        }
      } catch (error: unknown) {
        return {
          status: 500,
          body: { error: error instanceof Error ? error.message : 'Unknown error' },
        }
      }
    },
  )

  motia.registerApi(
    {
      method: 'POST',
      path: '/__motia/state',
    },
    async (req: ApiRequest, ctx: FlowContext): Promise<ApiResponse> => {
      try {
        const { key, groupId, value } = req.body as { key: string; groupId: string; value: any }
        await ctx.state.set(groupId, key, value)
        return {
          status: 200,
          body: { key, groupId, value },
        }
      } catch (error: unknown) {
        return {
          status: 500,
          body: { error: error instanceof Error ? error.message : 'Unknown error' },
        }
      }
    },
  )

  motia.registerApi(
    {
      method: 'POST',
      path: '/__motia/state/delete',
    },
    async (req: ApiRequest, ctx: FlowContext): Promise<ApiResponse> => {
      try {
        for (const id of (req.body as { ids: string[] }).ids) {
          const [groupId, ...keyParts] = id.split(':')
          const key = keyParts.join(':')
          await ctx.state.delete(groupId, key)
        }

        return {
          status: 204,
          body: '',
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
