import { getInstance, initIII } from '../../src/new/iii'
import { initTestEnv, TEST_API_URL, waitForReady, waitForRegistration } from './setup'

describe('api triggers integration', () => {
  beforeAll(async () => {
    initTestEnv()
    initIII({ enabled: false })
    const sdk = getInstance()
    await waitForReady(sdk)
  }, 15000)

  afterAll(async () => {
    const sdk = getInstance()
    await sdk.shutdown()
  })

  it('GET endpoint returns 200 with body', async () => {
    const sdk = getInstance()
    const functionId = `test.api.get.${Date.now()}`

    sdk.registerFunction({ id: functionId }, async () => ({
      status_code: 200,
      body: { message: 'Hello from GET' },
    }))
    sdk.registerTrigger({
      type: 'http',
      function_id: functionId,
      config: { api_path: 'test/hello', http_method: 'GET' },
    })

    await waitForRegistration(sdk, functionId)

    const res = await fetch(`${TEST_API_URL}/test/hello`)
    expect(res.status).toBe(200)
    const data = await res.json()
    expect(data).toEqual({ message: 'Hello from GET' })
  }, 10000)

  it('POST endpoint with body returns 201', async () => {
    const sdk = getInstance()
    const functionId = `test.api.post.${Date.now()}`

    sdk.registerFunction({ id: functionId }, async (req: { body?: unknown }) => ({
      status_code: 201,
      body: { received: req.body, created: true },
    }))
    sdk.registerTrigger({
      type: 'http',
      function_id: functionId,
      config: { api_path: 'test/items', http_method: 'POST' },
    })

    await waitForRegistration(sdk, functionId)

    const res = await fetch(`${TEST_API_URL}/test/items`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: 'test item', value: 123 }),
    })
    expect(res.status).toBe(201)
    const data = (await res.json()) as { created: boolean; received: { name: string; value: number } }
    expect(data.created).toBe(true)
    expect(data.received).toEqual({ name: 'test item', value: 123 })
  }, 10000)

  it('path params are passed to handler', async () => {
    const sdk = getInstance()
    const functionId = `test.api.getById.${Date.now()}`

    sdk.registerFunction({ id: functionId }, async (req: { path_params?: Record<string, string> }) => ({
      status_code: 200,
      body: { id: req.path_params?.id },
    }))
    sdk.registerTrigger({
      type: 'http',
      function_id: functionId,
      config: { api_path: 'test/items/:id', http_method: 'GET' },
    })

    await waitForRegistration(sdk, functionId)

    const res = await fetch(`${TEST_API_URL}/test/items/abc123`)
    expect(res.status).toBe(200)
    const data = await res.json()
    expect(data).toEqual({ id: 'abc123' })
  }, 10000)

  it('query params are passed to handler', async () => {
    const sdk = getInstance()
    const functionId = `test.api.search.${Date.now()}`

    sdk.registerFunction({ id: functionId }, async (req: { query_params?: Record<string, string | string[]> }) => ({
      status_code: 200,
      body: {
        query: req.query_params?.q,
        limit: req.query_params?.limit,
      },
    }))
    sdk.registerTrigger({
      type: 'http',
      function_id: functionId,
      config: { api_path: 'test/search', http_method: 'GET' },
    })

    await waitForRegistration(sdk, functionId)

    const res = await fetch(`${TEST_API_URL}/test/search?q=hello&limit=10`)
    expect(res.status).toBe(200)
    const data = (await res.json()) as { query: string; limit: string }
    expect(data.query).toBe('hello')
    expect(data.limit).toBe('10')
  }, 10000)

  it('custom status code 404', async () => {
    const sdk = getInstance()
    const functionId = `test.api.notfound.${Date.now()}`

    sdk.registerFunction({ id: functionId }, async () => ({
      status_code: 404,
      body: { error: 'Not found' },
    }))
    sdk.registerTrigger({
      type: 'http',
      function_id: functionId,
      config: { api_path: 'test/missing', http_method: 'GET' },
    })

    await waitForRegistration(sdk, functionId)

    const res = await fetch(`${TEST_API_URL}/test/missing`)
    expect(res.status).toBe(404)
    const data = await res.json()
    expect(data).toEqual({ error: 'Not found' })
  }, 10000)
})
