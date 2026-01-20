import type { ApiResponse, MotiaPluginContext } from '@motiadev/core'

export const api = ({ lockedData, registerApi }: MotiaPluginContext): void => {
  registerApi(
    {
      method: 'GET',
      path: '/__motia/streams',
    },
    async (): Promise<ApiResponse> => {
      try {
        const streams = lockedData.listStreams()
        const streamInfos = streams
          .filter((stream) => !stream.hidden && !stream.config.name.startsWith('__motia.'))
          .map((stream) => ({
            id: stream.config.name,
            name: stream.config.name,
            hidden: stream.hidden || false,
            filePath: stream.filePath,
          }))

        return {
          status: 200,
          body: { streams: streamInfos },
        }
      } catch (error: unknown) {
        return {
          status: 500,
          body: { error: error instanceof Error ? error.message : 'Unknown error' },
        }
      }
    },
  )

  registerApi(
    {
      method: 'GET',
      path: '/__motia/streams/all',
    },
    async (): Promise<ApiResponse> => {
      try {
        const streams = lockedData.listStreams()
        const streamInfos = streams.map((stream) => ({
          id: stream.config.name,
          name: stream.config.name,
          hidden: stream.hidden || false,
          filePath: stream.filePath,
        }))

        return {
          status: 200,
          body: { streams: streamInfos },
        }
      } catch (error: unknown) {
        return {
          status: 500,
          body: { error: error instanceof Error ? error.message : 'Unknown error' },
        }
      }
    },
  )

  registerApi(
    {
      method: 'GET',
      path: '/__motia/streams/details/:name',
    },
    async (request): Promise<ApiResponse> => {
      try {
        const streamName = request.pathParams?.name as string
        if (!streamName) {
          return {
            status: 400,
            body: { error: 'Stream name is required' },
          }
        }

        const stream = lockedData.getStreamByName(streamName)

        if (!stream) {
          return {
            status: 404,
            body: { error: `Stream '${streamName}' not found` },
          }
        }

        return {
          status: 200,
          body: {
            id: stream.config.name,
            name: stream.config.name,
            hidden: stream.hidden || false,
            filePath: stream.filePath,
            schema: stream.config.schema,
          },
        }
      } catch (error: unknown) {
        return {
          status: 500,
          body: { error: error instanceof Error ? error.message : 'Unknown error' },
        }
      }
    },
  )

  registerApi(
    {
      method: 'GET',
      path: '/__motia/streams/group/:name/:groupId',
    },
    async (request): Promise<ApiResponse> => {
      try {
        const streamName = request.pathParams?.name as string
        const groupId = request.pathParams?.groupId as string

        if (!streamName || !groupId) {
          return {
            status: 400,
            body: { error: 'Stream name and group ID are required' },
          }
        }

        const stream = lockedData.getStreamByName(streamName)

        if (!stream) {
          return {
            status: 404,
            body: { error: `Stream '${streamName}' not found` },
          }
        }

        try {
          const streamAdapter = stream.factory()
          const items = await streamAdapter.getGroup(groupId)

          return {
            status: 200,
            body: {
              streamName,
              groupId,
              items: items || [],
              count: items?.length || 0,
            },
          }
        } catch {
          return {
            status: 200,
            body: {
              streamName,
              groupId,
              items: [],
              count: 0,
            },
          }
        }
      } catch (error: unknown) {
        return {
          status: 500,
          body: { error: error instanceof Error ? error.message : 'Unknown error' },
        }
      }
    },
  )

  registerApi(
    {
      method: 'GET',
      path: '/__motia/streams/item/:name/:groupId/:itemId',
    },
    async (request): Promise<ApiResponse> => {
      try {
        const streamName = request.pathParams?.name as string
        const groupId = request.pathParams?.groupId as string
        const itemId = request.pathParams?.itemId as string

        if (!streamName || !groupId || !itemId) {
          return {
            status: 400,
            body: { error: 'Stream name, group ID, and item ID are required' },
          }
        }

        const stream = lockedData.getStreamByName(streamName)

        if (!stream) {
          return {
            status: 404,
            body: { error: `Stream '${streamName}' not found` },
          }
        }

        try {
          const streamAdapter = stream.factory()
          const item = await streamAdapter.get(groupId, itemId)

          return {
            status: 200,
            body: {
              streamName,
              groupId,
              itemId,
              data: item,
            },
          }
        } catch {
          return {
            status: 200,
            body: {
              streamName,
              groupId,
              itemId,
              data: null,
            },
          }
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
