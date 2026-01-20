export type ApiRouteMethod = 'GET' | 'POST' | 'PUT' | 'DELETE' | 'PATCH' | 'OPTIONS' | 'HEAD'
export type QueryParam = { name: string; description: string }

export type ApiEndpoint = {
  id: string
  method: ApiRouteMethod
  path: string
  description?: string
  queryParams?: QueryParam[]
  responseSchema?: Record<string, any> // eslint-disable-line @typescript-eslint/no-explicit-any
  bodySchema?: Record<string, Record<string, any>> // eslint-disable-line @typescript-eslint/no-explicit-any
  flows?: string[]
}
