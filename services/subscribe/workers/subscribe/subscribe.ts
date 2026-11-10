// The iii.dev email signup: add the address to Resend Contacts and send the
// welcome template once per address. index.ts binds it to POST /api/subscribe.
//
// A known address is re-subscribed but never sent a second welcome, so the
// public endpoint can't be used to mail the same inbox repeatedly. The send
// also carries an Idempotency-Key so a double submit can't race two welcomes.
//
// The welcome goes out before the contact is created: the contact is the
// record that the welcome was sent, so a failed send leaves no contact and the
// next submit tries again (the Idempotency-Key keeps a retry within 24 hours
// from sending twice).

import { createHash } from 'node:crypto'

const RESEND_API = 'https://api.resend.com'
const EMAIL_RE = /^[^\s@]+@[^\s@]+\.[^\s@]+$/
const MAX_BODY_BYTES = 1024
const WINDOW_MS = 60_000

export type Config = {
  apiKey: string
  templateId: string
  from: string
  subject: string
  /**
   * Where a signup came from (the body's `source`) to the Resend segment its contact joins. A
   * caller names a source, never a segment ID; a source not listed here is refused.
   */
  segments: Record<string, string>
  /** The source of a body that names none. */
  defaultSource: string
  allowedOrigins: string[]
  /** Most welcome emails sent per minute (per replica); new addresses past it get a 429. */
  maxWelcomesPerMinute?: number
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

/** The first value of a request header, matched case-insensitively. */
function header(req: Request, name: string): string | undefined {
  const key = Object.keys(req.headers).find((k) => k.toLowerCase() === name)
  const value = key ? req.headers[key] : undefined
  return Array.isArray(value) ? value[0] : value
}

/**
 * The request body as JSON, or null when it is too large or not JSON. The http
 * worker hands over a parsed JSON body; a raw string is parsed here.
 */
function parseBody(body: unknown): { email?: unknown; source?: unknown } | null {
  let parsed = body
  if (typeof body === 'string') {
    if (Buffer.byteLength(body) > MAX_BODY_BYTES) return null
    try {
      parsed = JSON.parse(body)
    } catch {
      return null
    }
  } else if (Buffer.byteLength(JSON.stringify(body ?? null)) > MAX_BODY_BYTES) {
    return null
  }
  return parsed !== null && typeof parsed === 'object' ? parsed : null
}

/** The normalized address, or null when it is not a valid one. */
function normalizeEmail(raw: unknown): string | null {
  const email = typeof raw === 'string' ? raw.trim().toLowerCase() : ''
  return email.length <= 254 && EMAIL_RE.test(email) ? email : null
}

/**
 * Fixed window counter: `take()` is false once `limit` calls landed in the
 * current minute. Origin is trivial to forge from a script, so this caps how
 * many new addresses the endpoint can mail, whoever is calling.
 */
function windowLimit(limit: number, now: () => number) {
  let start = 0
  let count = 0
  return function take(): boolean {
    const t = now()
    if (t - start >= WINDOW_MS) {
      start = t
      count = 0
    }
    if (count >= limit) return false
    count += 1
    return true
  }
}

/** Keys stay within Resend's 256-character limit whatever the address length. */
function welcomeKey(email: string): string {
  return `welcome/${createHash('sha256').update(email).digest('hex')}`
}

/** The POST /api/subscribe handler, bound to a Resend account and a `fetch`. */
export function createSubscribe(
  config: Config,
  fetch: typeof globalThis.fetch = globalThis.fetch,
  now: () => number = Date.now,
) {
  const allowedOrigins = new Set(config.allowedOrigins)
  const takeWelcome = windowLimit(config.maxWelcomesPerMinute ?? 20, now)

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

  /** Validate the request, then add the contact and send the welcome if the address is new. */
  return async function subscribe(req: Request): Promise<Response> {
    if (req.method.toUpperCase() !== 'POST') return reply(405, { error: 'method_not_allowed' })

    // Only the site's own forms may call this; blocks other pages from posting
    // signups through a visitor's browser.
    if (allowedOrigins.size > 0 && !allowedOrigins.has(header(req, 'origin') ?? '')) {
      return reply(403, { error: 'forbidden' })
    }

    const body = parseBody(req.body)
    const email = normalizeEmail(body?.email)
    if (!email) return reply(400, { error: 'invalid_email' })
    const source = body?.source ?? config.defaultSource
    if (typeof source !== 'string' || !Object.hasOwn(config.segments, source)) {
      return reply(400, { error: 'invalid_source' })
    }
    const segment = config.segments[source]

    try {
      const path = `/contacts/${encodeURIComponent(email)}`
      const existing = await resend('GET', path)

      if (existing.ok) {
        // Signing up again is fresh consent: clear an earlier unsubscribe, but
        // don't send the welcome twice.
        const contact = (await existing.json()) as { unsubscribed?: boolean }
        if (contact.unsubscribed) await ensure(await resend('PATCH', path, { unsubscribed: false }), 'update contact')
        // Someone already on the list who signs up from another source joins
        // that source's segment too. The signup itself already succeeded, so a
        // failure here is logged, not answered.
        if (segment) {
          const joined = await resend('POST', `${path}/segments/${encodeURIComponent(segment)}`)
          if (!joined.ok)
            console.error(`add to segment (${source}) failed: ${joined.status} ${await joined.text().catch(() => '')}`)
        }
        return reply(200, { ok: true })
      }
      if (existing.status !== 404) await ensure(existing, 'get contact')

      if (!takeWelcome()) return reply(429, { error: 'rate_limited' })

      const send = await resend(
        'POST',
        '/emails',
        { from: config.from, to: email, subject: config.subject, template: { id: config.templateId } },
        { 'idempotency-key': welcomeKey(email) },
      )
      await ensure(send, 'send welcome')

      const contact: Record<string, unknown> = { email, unsubscribed: false }
      if (segment) contact.segments = [{ id: segment }]
      await ensure(await resend('POST', '/contacts', contact), 'create contact')
      return reply(200, { ok: true })
    } catch (err) {
      console.error(err)
      return reply(502, { error: 'upstream_failed' })
    }
  }
}
