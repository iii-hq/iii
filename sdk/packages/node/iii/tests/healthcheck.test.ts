import type { HttpRequest, HttpResponse } from '@iii-dev/helpers/http'
import { describe, expect, it } from 'vitest'
import { execute, httpRequest, iii } from './utils'

describe('Healthcheck Endpoint', () => {
  it('serves a healthcheck function observing http::on-request', async () => {
    const fn = iii.registerFunction('test.healthcheck', async (_req: HttpRequest): Promise<HttpResponse> => {
      return {
        status_code: 200,
        body: {
          status: 'healthy',
          timestamp: new Date().toISOString(),
          service: 'iii-sdk-test',
        },
      }
    })

    await execute(async () => {
      const response = await httpRequest('GET', '/health')
      expect(response.status).toBe(404)
    })

    const route = iii.observe({ function_id: 'http::on-request', args: { method: 'GET', path: '/health' } }, fn.id)

    await execute(async () => {
      const response = await httpRequest('GET', '/health')

      expect(response.status).toBe(200)
      expect(response.data).toHaveProperty('status', 'healthy')
      expect(response.data).toHaveProperty('service', 'iii-sdk-test')
      expect(response.data).toHaveProperty('timestamp')
    })

    fn.unregister()
    await route.dispose()

    await execute(() => expect(httpRequest('GET', '/health')).resolves.toHaveProperty('status', 404))
  })
})
