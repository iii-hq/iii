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

const DB = 'primary'

// The database holds the durable record; iii-state is the hot lookup cache.
async function ensureSchema(): Promise<void> {
  await worker.trigger({
    function_id: 'database::execute',
    payload: {
      db: DB,
      sql: 'CREATE TABLE IF NOT EXISTS links (code TEXT PRIMARY KEY, url TEXT NOT NULL, created_at TEXT NOT NULL)',
    },
  })
  await worker.trigger({
    function_id: 'database::execute',
    payload: {
      db: DB,
      sql: 'CREATE TABLE IF NOT EXISTS clicks (id INTEGER PRIMARY KEY AUTOINCREMENT, code TEXT NOT NULL, clicked_at TEXT NOT NULL)',
    },
  })
}

worker.registerFunction('link::create', async (payload: { url: string; code?: string }) => {
  const code = payload.code ?? makeCode()
  await worker.trigger({
    function_id: 'database::execute',
    payload: {
      db: DB,
      sql: 'INSERT INTO links (code, url, created_at) VALUES (?, ?, ?)',
      params: [code, payload.url, new Date().toISOString()],
    },
  })
  await worker.trigger({
    function_id: 'state::set',
    payload: { scope: 'links', key: code, value: { url: payload.url } },
  })
  logger.info('link created', { code, url: payload.url })
  return { code, url: payload.url }
})

worker.registerFunction('link::resolve', async (payload: { code: string }) => {
  // Hot path: read the cache in iii-state.
  const cached = await worker.trigger<{ scope: string; key: string }, { url: string } | null>({
    function_id: 'state::get',
    payload: { scope: 'links', key: payload.code },
  })
  if (cached) {
    return { url: cached.url }
  }
  // Cache miss: fall back to the durable table, then warm the cache.
  const { rows } = await worker.trigger<
    { db: string; sql: string; params: string[] },
    { rows: Array<{ url: string }> }
  >({
    function_id: 'database::query',
    payload: { db: DB, sql: 'SELECT url FROM links WHERE code = ?', params: [payload.code] },
  })
  const url = rows[0]?.url ?? null
  if (url) {
    await worker.trigger({
      function_id: 'state::set',
      payload: { scope: 'links', key: payload.code, value: { url } },
    })
  }
  return { url }
})

// Record a click in the durable history. For now http::redirect calls this
// directly; the next chapter moves it onto a queue.
worker.registerFunction('link::record_click', async (payload: { code: string; clicked_at: string }) => {
  await worker.trigger({
    function_id: 'database::execute',
    payload: {
      db: DB,
      sql: 'INSERT INTO clicks (code, clicked_at) VALUES (?, ?)',
      params: [payload.code, payload.clicked_at],
    },
  })
  return { recorded: true }
})

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
  // Record the click as durable history. For now this is a direct call on the
  // redirect's hot path; the next chapter moves it onto a queue.
  await worker.trigger({
    function_id: 'link::record_click',
    payload: { code, clicked_at: new Date().toISOString() },
  })
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

ensureSchema()
  .then(() => logger.info('link worker ready'))
  .catch((err) => logger.error('schema init failed', { error: String(err) }))
