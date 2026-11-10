// The iii.dev email signup, with double opt-in. index.ts binds the handlers:
//
// - POST /api/subscribe (`subscribe`): emails the address a confirmation link.
//   Nothing is stored yet, so a typo or someone else's address never reaches
//   the list.
// - GET /api/subscribe/confirm (`confirm`): the link's target. It checks the
//   link, adds the contact to its source's segment, sends the welcome, and
//   redirects to /subscribed (or /subscribe-expired when the link is bad).
//
// The link carries the address and source, signed with a key derived from the
// Resend API key, and expires: no database holds pending signups. An address
// already on the list is sent nothing, so the public endpoint can't be used to
// mail the same inbox repeatedly; confirmation emails are also capped per
// minute. The welcome goes out once per address (Idempotency-Key), before the
// contact is created: the contact is the record that the welcome was sent.

import { createHash, createHmac, timingSafeEqual } from 'node:crypto'

const RESEND_API = 'https://api.resend.com'
const EMAIL_RE = /^[^\s@]+@[^\s@]+\.[^\s@]+$/
const MAX_BODY_BYTES = 1024
const MAX_TOKEN_CHARS = 1024
const WINDOW_MS = 60_000
const DAY_MS = 86_400_000

export type Config = {
  apiKey: string
  from: string
  /** The welcome, sent once the address is confirmed. */
  welcome: { templateId: string; subject: string }
  /**
   * The confirmation email. Its template reads the link from the `CONFIRM_URL` variable; `url` is
   * the confirm route's public address (the token is added as `?token=`).
   */
  confirmation: { templateId: string; subject: string; url: string; ttlMs?: number }
  /** Where a click on the link ends up. */
  redirects: { confirmed: string; expired: string }
  /**
   * Where a signup came from (the body's `source`) to the Resend segment its contact joins. A
   * caller names a source, never a segment ID; a source not listed here is refused.
   */
  segments: Record<string, string>
  /** The source of a body that names none. */
  defaultSource: string
  allowedOrigins: string[]
  /** Most confirmation emails sent per minute (per replica); new addresses past it get a 429. */
  maxEmailsPerMinute?: number
}

export type Request = {
  method: string
  headers: Record<string, string | string[] | undefined>
  query_params?: Record<string, string | string[] | undefined>
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

function redirect(location: string): Response {
  return { status_code: 302, headers: { location, 'cache-control': 'no-store' }, body: {} }
}

/** The first value of a header (matched case-insensitively) or a query parameter. */
function first(values: Record<string, string | string[] | undefined> | undefined, name: string): string | undefined {
  const key = Object.keys(values ?? {}).find((k) => k.toLowerCase() === name)
  const value = key ? values?.[key] : undefined
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

type Pending = { email: string; source: string }

/**
 * Signed, expiring confirmation tokens: `<payload>.<mac>`, both base64url, the
 * payload `{ e: email, s: source, x: expiry in ms }`. The key is derived from
 * the Resend API key, so a new key also retires every link sent before it.
 */
function tokens(apiKey: string, now: () => number) {
  const key = createHmac('sha256', apiKey).update('iii-subscribe/confirm-link/v1').digest()
  const mac = (payload: string) => createHmac('sha256', key).update(payload).digest()

  return {
    sign(pending: Pending, ttlMs: number): string {
      const payload = Buffer.from(JSON.stringify({ e: pending.email, s: pending.source, x: now() + ttlMs })).toString(
        'base64url',
      )
      return `${payload}.${mac(payload).toString('base64url')}`
    },
    /** The pending signup, or null when the token is malformed, forged or expired. */
    verify(token: string | undefined): Pending | null {
      if (!token || token.length > MAX_TOKEN_CHARS) return null
      const [payload, sig, extra] = token.split('.')
      if (!payload || !sig || extra !== undefined) return null
      const given = Buffer.from(sig, 'base64url')
      const expected = mac(payload)
      if (given.length !== expected.length || !timingSafeEqual(given, expected)) return null
      try {
        const { e, s, x } = JSON.parse(Buffer.from(payload, 'base64url').toString('utf8'))
        const email = normalizeEmail(e)
        if (!email || typeof s !== 'string' || typeof x !== 'number' || x < now()) return null
        return { email, source: s }
      } catch {
        return null
      }
    },
  }
}

/** The signup handlers, bound to a Resend account, a `fetch` and a clock. */
export function createSubscribe(
  config: Config,
  fetch: typeof globalThis.fetch = globalThis.fetch,
  now: () => number = Date.now,
) {
  const allowedOrigins = new Set(config.allowedOrigins)
  const takeEmail = windowLimit(config.maxEmailsPerMinute ?? 20, now)
  const links = tokens(config.apiKey, now)
  const ttlMs = config.confirmation.ttlMs ?? 3 * DAY_MS

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

  /** The contact, or null when Resend has none for the address. */
  async function findContact(email: string): Promise<{ unsubscribed?: boolean } | null> {
    const res = await resend('GET', `/contacts/${encodeURIComponent(email)}`)
    if (res.status === 404) return null
    await ensure(res, 'get contact')
    return (await res.json()) as { unsubscribed?: boolean }
  }

  /** Adds a known contact to a segment. Its signup already succeeded, so a failure is logged, not answered. */
  async function joinSegment(email: string, source: string) {
    const segment = config.segments[source]
    if (!segment) return
    const res = await resend('POST', `/contacts/${encodeURIComponent(email)}/segments/${encodeURIComponent(segment)}`)
    if (!res.ok) console.error(`add to segment (${source}) failed: ${res.status} ${await res.text().catch(() => '')}`)
  }

  /** POST /api/subscribe: validate, then email a confirmation link unless the address is already subscribed. */
  async function subscribe(req: Request): Promise<Response> {
    if (req.method.toUpperCase() !== 'POST') return reply(405, { error: 'method_not_allowed' })

    // Only the site's own forms may call this; blocks other pages from posting
    // signups through a visitor's browser.
    if (allowedOrigins.size > 0 && !allowedOrigins.has(first(req.headers, 'origin') ?? '')) {
      return reply(403, { error: 'forbidden' })
    }

    const body = parseBody(req.body)
    const email = normalizeEmail(body?.email)
    if (!email) return reply(400, { error: 'invalid_email' })
    const source = body?.source ?? config.defaultSource
    if (typeof source !== 'string' || !Object.hasOwn(config.segments, source)) {
      return reply(400, { error: 'invalid_source' })
    }

    try {
      const contact = await findContact(email)
      // Already confirmed: nothing to confirm, and no email. A new source still
      // adds its segment. The answer is the same either way, so it doesn't tell
      // a caller whether the address is on the list.
      if (contact && !contact.unsubscribed) {
        await joinSegment(email, source)
        return reply(200, { ok: true })
      }

      if (!takeEmail()) return reply(429, { error: 'rate_limited' })

      const link = `${config.confirmation.url}?token=${links.sign({ email, source }, ttlMs)}`
      await ensure(
        await resend('POST', '/emails', {
          from: config.from,
          to: email,
          subject: config.confirmation.subject,
          template: { id: config.confirmation.templateId, variables: { CONFIRM_URL: link } },
        }),
        'send confirmation',
      )
      return reply(200, { ok: true })
    } catch (err) {
      console.error(err)
      return reply(502, { error: 'upstream_failed' })
    }
  }

  /** GET /api/subscribe/confirm?token=: the link in the confirmation email. */
  async function confirm(req: Request): Promise<Response> {
    if (req.method.toUpperCase() !== 'GET') return reply(405, { error: 'method_not_allowed' })

    const pending = links.verify(first(req.query_params, 'token'))
    if (!pending) return redirect(config.redirects.expired)
    const { email, source } = pending

    try {
      const contact = await findContact(email)
      if (contact) {
        // Confirming again is fresh consent: clear an earlier unsubscribe. The
        // welcome went out when the contact was created, so not again.
        if (contact.unsubscribed) {
          const path = `/contacts/${encodeURIComponent(email)}`
          await ensure(await resend('PATCH', path, { unsubscribed: false }), 'update contact')
        }
        await joinSegment(email, source)
        return redirect(config.redirects.confirmed)
      }

      await ensure(
        await resend(
          'POST',
          '/emails',
          {
            from: config.from,
            to: email,
            subject: config.welcome.subject,
            template: { id: config.welcome.templateId },
          },
          { 'idempotency-key': welcomeKey(email) },
        ),
        'send welcome',
      )

      const created: Record<string, unknown> = { email, unsubscribed: false }
      const segment = config.segments[source]
      if (segment) created.segments = [{ id: segment }]
      await ensure(await resend('POST', '/contacts', created), 'create contact')
      return redirect(config.redirects.confirmed)
    } catch (err) {
      // The link is fine but Resend is not: the expired page asks for a new
      // link, which then works once Resend does.
      console.error(err)
      return redirect(config.redirects.expired)
    }
  }

  return { subscribe, confirm }
}
