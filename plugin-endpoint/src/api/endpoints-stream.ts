import {
  type ApiRouteConfig,
  type ApiRouteMethod,
  type JsonSchema,
  type LockedData,
  type QueryParam,
  type Step,
  StreamAdapter,
} from '@motiadev/core'

type ApiEndpoint = {
  id: string
  method: ApiRouteMethod
  path: string
  description?: string
  queryParams?: QueryParam[]
  responseSchema?: JsonSchema
  bodySchema?: JsonSchema
  flows?: string[]
}

export const mapEndpoint = (step: Step<ApiRouteConfig>): ApiEndpoint => {
  return {
    id: step.filePath,
    method: step.config.method,
    path: step.config.path,
    description: step.config.description,
    queryParams: step.config.queryParams,
    responseSchema: step.config.responseSchema as never as JsonSchema,
    bodySchema: step.config.bodySchema as never as JsonSchema,
    flows: step.config.flows,
  }
}

export class EndpointsStream extends StreamAdapter<ApiEndpoint> {
  constructor(private readonly lockedData: LockedData) {
    super('__motia.api-endpoints')
  }

  async get(_groupId: string, id: string): Promise<ApiEndpoint | null> {
    const endpoint = this.lockedData.apiSteps().find((step) => step.filePath === id)
    return endpoint ? mapEndpoint(endpoint) : null
  }

  async delete(id: string): Promise<ApiEndpoint> {
    return { id } as never
  }

  async set(_: string, __: string, data: ApiEndpoint): Promise<ApiEndpoint> {
    return data
  }

  async getGroup(): Promise<ApiEndpoint[]> {
    return this.lockedData.apiSteps().map(mapEndpoint)
  }
}
