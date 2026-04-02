import { useApi } from './hooks'

useApi(
  {
    api_path: 'http-fetch',
    http_method: 'GET',
    description: 'Fetch a todo from JSONPlaceholder (demonstrates OTel fetch instrumentation)',
    metadata: { tags: ['http-example'] },
  },
  async (_req, logger) => {
    logger.info('Fetching todo from external API')

    const response = await fetch('https://jsonplaceholder.typicode.com/todos/1')
    const data = await response.json()

    logger.info('Fetched todo successfully', { status: response.status })

    return {
      status_code: 200,
      body: { upstream_status: response.status, data },
      headers: { 'Content-Type': 'application/json' },
    }
  },
)

useApi(
  {
    api_path: 'http-fetch',
    http_method: 'POST',
    description: 'POST to httpbin (demonstrates request body size in OTel spans)',
    metadata: { tags: ['http-example'] },
  },
  async (req, logger) => {
    logger.info('Posting to httpbin', { body: req.body })

    const payload = JSON.stringify(req.body ?? { message: 'hello from iii' })
    const response = await fetch('https://httpbin.org/post', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: payload,
    })
    const data = await response.json()

    logger.info('Post completed', { status: response.status })

    return {
      status_code: response.status,
      body: { upstream_status: response.status, data },
      headers: { 'Content-Type': 'application/json' },
    }
  },
)
