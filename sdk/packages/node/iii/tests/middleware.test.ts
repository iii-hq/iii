import { describe, expect, it } from 'vitest'
import type { ApiResponse, HttpRequest } from '../src'
import { engineHttpUrl, execute, httpRequest, iii, sleep } from './utils'

describe('HTTP Middleware', () => {
  it('should execute middleware that continues to handler', async () => {
    let middlewareCalled = false

    const mwFn = iii.registerFunction('test.mw.continue', async (_req) => {
      middlewareCalled = true
      return { action: 'continue' }
    })

    const handlerFn = iii.registerFunction(
      'test.mw.continue.handler',
      async (_req: HttpRequest): Promise<ApiResponse> => ({
        status_code: 200,
        body: { message: 'handler reached' },
      }),
    )

    const trigger = iii.registerTrigger({
      type: 'http',
      function_id: handlerFn.id,
      config: {
        api_path: 'test/mw/continue',
        http_method: 'GET',
        middleware_function_ids: ['test.mw.continue'],
      },
    })

    await sleep(300)

    const response = await execute(async () => httpRequest('GET', '/test/mw/continue'))

    expect(response.status).toBe(200)
    expect(response.data).toEqual({ message: 'handler reached' })
    expect(middlewareCalled).toBe(true)

    mwFn.unregister()
    handlerFn.unregister()
    trigger.unregister()
  })

  it('should short-circuit when middleware responds', async () => {
    let handlerCalled = false

    const mwFn = iii.registerFunction('test.mw.block', async (_req) => ({
      action: 'respond',
      response: {
        status_code: 403,
        body: { error: 'Forbidden by middleware' },
      },
    }))

    const handlerFn = iii.registerFunction(
      'test.mw.block.handler',
      async (_req: HttpRequest): Promise<ApiResponse> => {
        handlerCalled = true
        return { status_code: 200, body: { message: 'should not reach' } }
      },
    )

    const trigger = iii.registerTrigger({
      type: 'http',
      function_id: handlerFn.id,
      config: {
        api_path: 'test/mw/block',
        http_method: 'GET',
        middleware_function_ids: ['test.mw.block'],
      },
    })

    await sleep(300)

    const response = await httpRequest('GET', '/test/mw/block')

    expect(response.status).toBe(403)
    expect(response.data).toEqual({ error: 'Forbidden by middleware' })
    expect(handlerCalled).toBe(false)

    mwFn.unregister()
    handlerFn.unregister()
    trigger.unregister()
  })

  it('should execute multiple middleware in order', async () => {
    const callOrder: string[] = []

    const mw1 = iii.registerFunction('test.mw.order.first', async (_req) => {
      callOrder.push('mw1')
      return { action: 'continue' }
    })

    const mw2 = iii.registerFunction('test.mw.order.second', async (_req) => {
      callOrder.push('mw2')
      return { action: 'continue' }
    })

    const handlerFn = iii.registerFunction(
      'test.mw.order.handler',
      async (_req: HttpRequest): Promise<ApiResponse> => {
        callOrder.push('handler')
        return { status_code: 200, body: { order: callOrder } }
      },
    )

    const trigger = iii.registerTrigger({
      type: 'http',
      function_id: handlerFn.id,
      config: {
        api_path: 'test/mw/order',
        http_method: 'GET',
        middleware_function_ids: ['test.mw.order.first', 'test.mw.order.second'],
      },
    })

    await sleep(300)

    const response = await execute(async () => httpRequest('GET', '/test/mw/order'))

    expect(response.status).toBe(200)
    expect(response.data.order).toEqual(['mw1', 'mw2', 'handler'])

    mw1.unregister()
    mw2.unregister()
    handlerFn.unregister()
    trigger.unregister()
  })

  it('should pass request metadata to middleware', async () => {
    let receivedReq: Record<string, unknown> = {}

    const mwFn = iii.registerFunction('test.mw.meta', async (req) => {
      receivedReq = req
      return { action: 'continue' }
    })

    const handlerFn = iii.registerFunction(
      'test.mw.meta.handler',
      async (_req: HttpRequest): Promise<ApiResponse> => ({
        status_code: 200,
        body: { ok: true },
      }),
    )

    const trigger = iii.registerTrigger({
      type: 'http',
      function_id: handlerFn.id,
      config: {
        api_path: 'test/mw/meta/:id',
        http_method: 'GET',
        middleware_function_ids: ['test.mw.meta'],
      },
    })

    await sleep(300)

    await execute(async () => httpRequest('GET', '/test/mw/meta/42?q=hello'))

    expect(receivedReq).toHaveProperty('phase', 'preHandler')
    expect(receivedReq).toHaveProperty('request')
    const request = receivedReq.request as Record<string, unknown>
    expect(request).toHaveProperty('method', 'GET')
    expect(request).toHaveProperty('headers')
    expect((request.path_params as Record<string, string>)?.id).toBe('42')
    expect((request.query_params as Record<string, string>)?.q).toBe('hello')

    mwFn.unregister()
    handlerFn.unregister()
    trigger.unregister()
  })

  it('should work without middleware (regression)', async () => {
    const fn = iii.registerFunction(
      'test.mw.none',
      async (_req: HttpRequest): Promise<ApiResponse> => ({
        status_code: 200,
        body: { message: 'no middleware' },
      }),
    )

    const trigger = iii.registerTrigger({
      type: 'http',
      function_id: fn.id,
      config: {
        api_path: 'test/mw/none',
        http_method: 'GET',
      },
    })

    await sleep(300)

    const response = await execute(async () => httpRequest('GET', '/test/mw/none'))

    expect(response.status).toBe(200)
    expect(response.data).toEqual({ message: 'no middleware' })

    fn.unregister()
    trigger.unregister()
  })
})
