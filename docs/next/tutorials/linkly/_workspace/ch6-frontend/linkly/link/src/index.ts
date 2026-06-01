import { registerWorker, Logger, TriggerAction } from 'iii-sdk'


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
      sql: 'CREATE TABLE IF NOT EXISTS links (code TEXT PRIMARY KEY, url TEXT NOT NULL, created_at TEXT NOT NULL, expires_at TEXT)',
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

worker.registerFunction(
  'link::create',
  async (payload: { url: string; code?: string; expires_at?: string }) => {
    const code = payload.code ?? makeCode()
    await worker.trigger({
      function_id: 'database::execute',
      payload: {
        db: DB,
        sql: 'INSERT INTO links (code, url, created_at, expires_at) VALUES (?, ?, ?, ?)',
        params: [code, payload.url, new Date().toISOString(), payload.expires_at ?? null],
      },
    })
    await worker.trigger({
      function_id: 'state::set',
      payload: { scope: 'links', key: code, value: { url: payload.url } },
    })
    await worker.trigger({ function_id: 'publish', payload: { topic: 'link.created', data: { code, url: payload.url } } })
    logger.info('link created', { code, url: payload.url })
    return { code, url: payload.url }
  },
)

worker.registerFunction('link::update', async (payload: { code: string; url: string }) => {
  await worker.trigger({
    function_id: 'database::execute',
    payload: {
      db: DB,
      sql: 'UPDATE links SET url = ? WHERE code = ?',
      params: [payload.url, payload.code],
    },
  })
  await worker.trigger({ function_id: 'iii::durable::publish', payload: { topic: 'link.updated', data: { code: payload.code, url: payload.url } } })
  return { code: payload.code, url: payload.url }
})

worker.registerFunction('link::delete', async (payload: { code: string }) => {
  await worker.trigger({
    function_id: 'database::execute',
    payload: { db: DB, sql: 'DELETE FROM links WHERE code = ?', params: [payload.code] },
  })
  await worker.trigger({ function_id: 'state::delete', payload: { scope: 'links', key: payload.code } })
  logger.info('link deleted', { code: payload.code })
  return { deleted: true }
})

// Ask a connected browser to confirm, then delete only if it says yes. The
// browser registers user::confirm_destructive_op; this calls it and waits.
worker.registerFunction('link::request_delete', async (payload: { code: string }) => {
  const { confirmed } = await worker.trigger<
    { code: string; action: string },
    { confirmed: boolean }
  >({
    function_id: 'user::confirm_destructive_op',
    payload: { code: payload.code, action: `delete link "${payload.code}"` },
  })
  if (!confirmed) {
    return { deleted: false }
  }
  await worker.trigger({ function_id: 'link::delete', payload: { code: payload.code } })
  return { deleted: true }
})

// Browser RBAC gate. Runs on every connection to the :3110 worker-manager.
// Validates a token from the query string and returns the session's allow-list.
// In production, look the token up in your own store.
worker.registerFunction(
  'link::auth_browser',
  async (input: { headers: Record<string, string>; query_params: Record<string, string[]>; ip_address: string }) => {
    const token = input.query_params.token?.[0]
    if (!token || token !== (process.env.LINKLY_BROWSER_TOKEN ?? 'dev-token')) {
      throw new Error('unauthorized')
    }
    return {
      allowed_functions: [],
      forbidden_functions: [],
      allow_trigger_type_registration: false,
      allow_function_registration: true,
      context: { source: 'browser' },
    }
  },
)

worker.registerFunction('link::resolve', async (payload: { code: string }) => {
  const cached = await worker.trigger<{ scope: string; key: string }, { url: string } | null>({
    function_id: 'state::get',
    payload: { scope: 'links', key: payload.code },
  })
  if (cached) {
    return { url: cached.url }
  }
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

// Queue consumer: write the click to the durable history, and publish it to the
// live click stream so subscribers (e.g. a browser) see it in real time.
worker.registerFunction('link::record_click', async (payload: { code: string; clicked_at: string }) => {
  await worker.trigger({
    function_id: 'database::execute',
    payload: {
      db: DB,
      sql: 'INSERT INTO clicks (code, clicked_at) VALUES (?, ?)',
      params: [payload.code, payload.clicked_at],
    },
  })
  const clickId = `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
  await worker.trigger({
    function_id: 'stream::set',
    payload: {
      stream_name: 'clicks',
      group_id: 'all',
      item_id: clickId,
      data: { code: payload.code, clicked_at: payload.clicked_at },
    },
  })
  return { recorded: true }
})

// Cron: nightly sweep of expired links, removed from both the database and the cache.
worker.registerFunction('link::sweep_expired', async () => {
  const now = new Date().toISOString()
  const { rows } = await worker.trigger<
    { db: string; sql: string; params: string[] },
    { rows: Array<{ code: string }> }
  >({
    function_id: 'database::query',
    payload: { db: DB, sql: 'SELECT code FROM links WHERE expires_at IS NOT NULL AND expires_at < ?', params: [now] },
  })
  for (const { code } of rows) {
    await worker.trigger({ function_id: 'state::delete', payload: { scope: 'links', key: code } })
  }
  await worker.trigger({
    function_id: 'database::execute',
    payload: { db: DB, sql: 'DELETE FROM links WHERE expires_at IS NOT NULL AND expires_at < ?', params: [now] },
  })
  logger.info('swept expired links', { count: rows.length })
  return { swept: rows.length }
})

worker.registerTrigger({
  type: 'cron',
  function_id: 'link::sweep_expired',
  config: { expression: '0 0 3 * * *' },
})

// Cron: daily digest of the most-clicked links.
worker.registerFunction('link::daily_digest', async () => {
  const { rows } = await worker.trigger<
    { db: string; sql: string },
    { rows: Array<{ code: string; clicks: number }> }
  >({
    function_id: 'database::query',
    payload: {
      db: DB,
      sql: 'SELECT code, COUNT(*) AS clicks FROM clicks GROUP BY code ORDER BY clicks DESC LIMIT 5',
    },
  })
  logger.info('daily digest: top links', { top: rows })
  return { top: rows }
})

worker.registerTrigger({
  type: 'cron',
  function_id: 'link::daily_digest',
  config: { expression: '0 0 9 * * *' },
})

worker.registerFunction('link::on_link_updated', async (data: { code: string; url: string }) => {
  await worker.trigger({
    function_id: 'state::set',
    payload: { scope: 'links', key: data.code, value: { url: data.url } },
  })
})

worker.registerTrigger({
  type: 'durable:subscriber',
  function_id: 'link::on_link_updated',
  config: { topic: 'link.updated' },
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
  await worker.trigger({
    function_id: 'link::record_click',
    payload: { code, clicked_at: new Date().toISOString() },
    action: TriggerAction.Enqueue({ queue: 'clicks' }),
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
    const { url, code, expires_at } = req.body ?? {}
    if (!url) {
      return {
        status_code: 400,
        body: { error: 'missing "url"' },
        headers: { 'Content-Type': 'application/json' },
      }
    }
    const link = await worker.trigger<
      { url: string; code?: string; expires_at?: string },
      { code: string; url: string }
    >({
      function_id: 'link::create',
      payload: { url, code, expires_at },
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

worker.registerFunction(
  'http::update',
  async (req) => {
    const code = req.path_params.code
    const url = req.body?.url
    if (!url) {
      return {
        status_code: 400,
        body: { error: 'missing "url"' },
        headers: { 'Content-Type': 'application/json' },
      }
    }
    const link = await worker.trigger<{ code: string; url: string }, { code: string; url: string }>({
      function_id: 'link::update',
      payload: { code, url },
    })
    return {
      status_code: 200,
      body: link,
      headers: { 'Content-Type': 'application/json' },
    }
  },
)

worker.registerTrigger({
  type: 'http',
  function_id: 'http::update',
  config: { api_path: '/links/:code', http_method: 'PUT' },
})

ensureSchema()
  .then(() => logger.info('link worker ready'))
  .catch((err) => logger.error('schema init failed', { error: String(err) }))
