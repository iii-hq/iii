// The iii.dev email signup: add the address to Resend Contacts and send the
// welcome template once per address. index.ts binds it to POST /api/subscribe.
//
// A known address is re-subscribed but never sent a second welcome, so the
// public endpoint can't be used to mail the same inbox repeatedly. The send
// also carries an Idempotency-Key so a double submit can't race two welcomes.

const RESEND_API = 'https://api.resend.com'
const EMAIL_RE = /^[^\s@]+@[^\s@]+\.[^\s@]+$/
const MAX_BODY_BYTES = 1024

export type Config = {
  apiKey: string
  templateId: string
  from: string
  subject: string
  segmentId?: string
  allowedOrigins: string[]
}

export type Request = {
  method: string
  headers: Record<string, string | string[] | undefined>
  body: unknown
}

export type Response = {
  status_code: number
  headers: Record<string, string>
  body: Record<string, unknown>
}

function reply(status_code: number, body: Record<string, unknown>): Response {
  return { status_code, headers: { 'content-type': 'application/json', 'cache-control': 'no-store' }, body }
}

function header(req: Request, name: string): string | undefined {
  const key = Object.keys(req.headers).find((k) => k.toLowerCase() === name)
  const value = key ? req.headers[key] : undefined
  return Array.isArray(value) ? value[0] : value
}

// The http worker hands over a parsed JSON body; a raw string is parsed here.
function parseEmail(body: unknown): string | null {
  let parsed = body
  if (typeof body === 'string') {
    if (Buffer.byteLength(body) > MAX_BODY_BYTES) return null
    try {
      parsed = JSON.parse(body)
    } catch {
      return null
    }
  } else if (JSON.stringify(body ?? null).length > MAX_BODY_BYTES) {
    return null
  }
  const raw = (parsed as { email?: unknown } | null)?.email
  const email = typeof raw === 'string' ? raw.trim().toLowerCase() : ''
  return email.length <= 254 && EMAIL_RE.test(email) ? email : null
}

export function createSubscribe(config: Config, fetch: typeof globalThis.fetch = globalThis.fetch) {
  const allowedOrigins = new Set(config.allowedOrigins)

  function resend(method: string, path: string, body?: unknown, headers: Record<string, string> = {}) {
    return fetch(`${RESEND_API}${path}`, {
      method,
      headers: { authorization: `Bearer ${config.apiKey}`, 'content-type': 'application/json', ...headers },
      body: body ? JSON.stringify(body) : undefined,
    })
  }

  async function ensure(res: globalThis.Response, what: string) {
    if (res.ok) return
    const detail = await res.text().catch(() => '')
    throw new Error(`${what} failed: ${res.status} ${detail}`)
  }

  return async function subscribe(req: Request): Promise<Response> {
    if (req.method.toUpperCase() !== 'POST') return reply(405, { error: 'method_not_allowed' })

    // Only the site's own forms may call this; blocks other pages from posting
    // signups through a visitor's browser.
    if (allowedOrigins.size > 0 && !allowedOrigins.has(header(req, 'origin') ?? '')) {
      return reply(403, { error: 'forbidden' })
    }

    const email = parseEmail(req.body)
    if (!email) return reply(400, { error: 'invalid_email' })

    try {
      const path = `/contacts/${encodeURIComponent(email)}`
      const existing = await resend('GET', path)

      if (existing.ok) {
        // Signing up again is fresh consent: clear an earlier unsubscribe, but
        // don't send the welcome twice.
        const contact = (await existing.json()) as { unsubscribed?: boolean }
        if (contact.unsubscribed) await ensure(await resend('PATCH', path, { unsubscribed: false }), 'update contact')
        return reply(200, { ok: true })
      }
      if (existing.status !== 404) await ensure(existing, 'get contact')

      const contact: Record<string, unknown> = { email, unsubscribed: false }
      if (config.segmentId) contact.segments = [{ id: config.segmentId }]
      await ensure(await resend('POST', '/contacts', contact), 'create contact')

      const send = await resend(
        'POST',
        '/emails',
        { from: config.from, to: email, subject: config.subject, template: { id: config.templateId } },
        { 'idempotency-key': `welcome/${email}` },
      )
      await ensure(send, 'send welcome')
      return reply(200, { ok: true })
    } catch (err) {
      console.error(err)
      return reply(502, { error: 'upstream_failed' })
    }
  }
}
