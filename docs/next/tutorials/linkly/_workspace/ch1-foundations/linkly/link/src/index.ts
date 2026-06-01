import { registerWorker, Logger } from 'iii-sdk'

const worker = registerWorker(process.env.III_URL ?? 'ws://localhost:49134', {
  workerName: 'link',
})
const logger = new Logger()

const CHARS = 'abcdefghijklmnopqrstuvwxyz0123456789'

function makeCode(): string {
  let s = ''
  for (let i = 0; i < 6; i++) s += CHARS[Math.floor(Math.random() * CHARS.length)]
  return s
}

// --- Domain functions: callable with `iii trigger` ---

worker.registerFunction('link::create', async (payload: { url: string; code?: string }) => {
  const code = payload.code ?? makeCode()
  await worker.trigger({
    function_id: 'state::set',
    payload: { scope: 'links', key: code, value: { url: payload.url } },
  })
  logger.info('link created', { code, url: payload.url })
  return { code, url: payload.url }
})

worker.registerFunction('link::resolve', async (payload: { code: string }) => {
  const stored = await worker.trigger<{ scope: string; key: string }, { url: string } | null>({
    function_id: 'state::get',
    payload: { scope: 'links', key: payload.code },
  })
  return { url: stored?.url ?? null }
})

// --- HTTP edge: added after `iii worker add iii-http` ---

worker.registerFunction('http::redirect', async (req) => {
  const code = req.path_params.code
  const { url } = await worker.trigger<{ code: string }, { url: string | null }>({
    function_id: 'link::resolve',
    payload: { code },
  })
  if (!url) {
    return {
      status_code: 404,
      body: { error: 'link not found' },
      headers: { 'Content-Type': 'application/json' },
    }
  }
  return { status_code: 302, headers: { Location: url } }
})

worker.registerTrigger({
  type: 'http',
  function_id: 'http::redirect',
  config: { api_path: '/s/:code', http_method: 'GET' },
})

worker.registerFunction(
  'http::create',
  async (req) => {
    const { url, code } = req.body ?? {}
    if (!url) {
      return {
        status_code: 400,
        body: { error: 'missing "url"' },
        headers: { 'Content-Type': 'application/json' },
      }
    }
    const link = await worker.trigger<{ url: string; code?: string }, { code: string; url: string }>({
      function_id: 'link::create',
      payload: { url, code },
    })
    return {
      status_code: 201,
      body: link,
      headers: { 'Content-Type': 'application/json' },
    }
  },
)

worker.registerTrigger({
  type: 'http',
  function_id: 'http::create',
  config: { api_path: '/links', http_method: 'POST' },
})

logger.info('link worker ready')
