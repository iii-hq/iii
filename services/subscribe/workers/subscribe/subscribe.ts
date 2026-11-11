// The iii.dev email signup, with double opt-in. index.ts binds the handlers:
//
// - POST /api/subscribe (`subscribe`): emails the address a confirmation link.
//   Nothing is stored yet, so a typo or someone else's address never reaches
//   the list.
// - POST /api/subscribe/confirm (`confirm`): the link opens iii.dev/subscribe-confirm
//   with the token after `#`, and its Confirm button posts the token here. This
//   adds the contact to its source's segment and sends the welcome. A button,
//   not the link itself, confirms: mail scanners that open every link can't
//   subscribe anyone, and a fragment never reaches a server log.
//
// The token is the pending signup (address, source, expiry) encrypted and
// authenticated with AES-256-GCM, under a key derived from the Resend API key:
// it hides the address, can't be forged or altered, and expires, so no database
// holds pending signups. An address already on the list is sent nothing, so the
// public endpoint can't mail the same inbox repeatedly; confirmation emails are
// also capped per minute. The welcome goes out once per address
// (Idempotency-Key), before the contact is created: the contact is the record
// that the welcome was sent.

import { createCipheriv, createDecipheriv, createHash, createHmac, randomBytes } from 'node:crypto'

const RESEND_API = 'https://api.resend.com'
const EMAIL_RE = /^[^\s@]+@[^\s@]+\.[^\s@]+$/
const MAX_BODY_BYTES = 1024
const MAX_TOKEN_CHARS = 1024
const IV_BYTES = 12
const TAG_BYTES = 16
const WINDOW_MS = 60_000
const DAY_MS = 86_400_000

export type Config = {
  apiKey: string
  from: string
  /** The welcome, sent once the address is confirmed. */
  welcome: { templateId: string; subject: string }
  /**
   * The confirmation email. Its template reads the link from the `CONFIRM_URL` variable; `url` is
   * the page that confirms (the token is added as `#token=`).
   */
  confirmation: { templateId: string; subject: string; url: string; ttlMs?: number }
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
function parseBody(body: unknown): { email?: unknown; source?: unknown; token?: unknown } | null {
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

/**
 * A signup waiting for its click. `resubscribe` says the address was on the list
 * but unsubscribed when the link was sent: only such a link may re-subscribe it.
 * The link of a first signup can't re-subscribe someone who unsubscribes later.
 */
type Pending = { email: string; source: string; resubscribe: boolean }

/**
 * Confirmation tokens: base64url of `iv || ciphertext || tag`, AES-256-GCM over
 * `{ e: email, s: source, r: resubscribe, x: expiry in ms }`. GCM both hides the
 * payload and refuses any token not made with the key. The key is HMAC-SHA256
 * of a fixed label under the Resend API key: one-way, so a token reveals
 * nothing about the API key, and a new API key retires every link sent before
 * it.
 */
function tokens(apiKey: string, now: () => number) {
  const key = createHmac('sha256', apiKey).update('iii-subscribe/confirm-token/v2').digest()

  return {
    seal(pending: Pending, ttlMs: number): string {
      const iv = randomBytes(IV_BYTES)
      const cipher = createCipheriv('aes-256-gcm', key, iv)
      const plain = JSON.stringify({ e: pending.email, s: pending.source, r: pending.resubscribe, x: now() + ttlMs })
      const sealed = Buffer.concat([cipher.update(plain, 'utf8'), cipher.final()])
      return Buffer.concat([iv, sealed, cipher.getAuthTag()]).toString('base64url')
    },
    /** The pending signup, or null when the token is malformed, forged, altered or expired. */
    open(token: unknown): Pending | null {
      if (typeof token !== 'string' || token.length > MAX_TOKEN_CHARS || !/^[A-Za-z0-9_-]+$/.test(token)) return null
      const raw = Buffer.from(token, 'base64url')
      if (raw.length <= IV_BYTES + TAG_BYTES) return null
      try {
        const decipher = createDecipheriv('aes-256-gcm', key, raw.subarray(0, IV_BYTES))
        decipher.setAuthTag(raw.subarray(raw.length - TAG_BYTES))
        const plain = Buffer.concat([decipher.update(raw.subarray(IV_BYTES, raw.length - TAG_BYTES)), decipher.final()])
        const { e, s, r, x } = JSON.parse(plain.toString('utf8'))
        const email = normalizeEmail(e)
        if (!email || typeof s !== 'string' || typeof x !== 'number' || x < now()) return null
        return { email, source: s, resubscribe: r === true }
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

  /** Method and origin: both routes are posted to by iii.dev pages only. */
  function refused(req: Request): Response | null {
    if (req.method.toUpperCase() !== 'POST') return reply(405, { error: 'method_not_allowed' })
    // Blocks other pages from posting through a visitor's browser.
    if (allowedOrigins.size > 0 && !allowedOrigins.has(header(req, 'origin') ?? '')) {
      return reply(403, { error: 'forbidden' })
    }
    return null
  }

  /** POST /api/subscribe `{ email, source? }`: email a confirmation link unless the address is subscribed. */
  async function subscribe(req: Request): Promise<Response> {
    const no = refused(req)
    if (no) return no

    const body = parseBody(req.body)
    const email = normalizeEmail(body?.email)
    if (!email) return reply(400, { error: 'invalid_email' })
    const source = body?.source ?? config.defaultSource
    if (typeof source !== 'string' || !Object.hasOwn(config.segments, source)) {
      return reply(400, { error: 'invalid_source' })
    }

    try {
      const contact = await findContact(email)
      // Already subscribed: nothing changes and nothing is sent, whatever the
      // source (a segment is where someone first subscribed, and joining
      // another would need its own consent). The answer is the same either
      // way, so it doesn't tell a caller whether the address is on the list.
      if (contact && !contact.unsubscribed) return reply(200, { ok: true })

      if (!takeEmail()) return reply(429, { error: 'rate_limited' })

      const token = links.seal({ email, source, resubscribe: contact !== null }, ttlMs)
      await ensure(
        await resend('POST', '/emails', {
          from: config.from,
          to: email,
          subject: config.confirmation.subject,
          template: {
            id: config.confirmation.templateId,
            variables: { CONFIRM_URL: `${config.confirmation.url}#token=${token}` },
          },
        }),
        'send confirmation',
      )
      return reply(200, { ok: true })
    } catch (err) {
      console.error(err)
      return reply(502, { error: 'upstream_failed' })
    }
  }

  /**
   * POST /api/subscribe/confirm `{ token }`, from the Confirm button of iii.dev/subscribe-confirm.
   * `{ ok: true }` when the address is subscribed (also on a second click); a 400 `invalid_link`
   * for a bad, altered or expired token, or one that can't re-subscribe this address.
   */
  async function confirm(req: Request): Promise<Response> {
    const no = refused(req)
    if (no) return no

    const pending = links.open(parseBody(req.body)?.token)
    if (!pending) return reply(400, { error: 'invalid_link' })
    const { email, source } = pending
    const path = `/contacts/${encodeURIComponent(email)}`

    try {
      const contact = await findContact(email)
      if (contact && !contact.unsubscribed) return reply(200, { ok: true })

      if (contact) {
        // Unsubscribed since: only a link sent while unsubscribed (a fresh
        // signup) brings the address back, not an older one. The welcome went
        // out when the contact was created, so not again.
        if (!pending.resubscribe) return reply(400, { error: 'invalid_link' })
        await ensure(await resend('PATCH', path, { unsubscribed: false }), 'update contact')
        return reply(200, { ok: true })
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
      return reply(200, { ok: true })
    } catch (err) {
      console.error(err)
      return reply(502, { error: 'upstream_failed' })
    }
  }

  return { subscribe, confirm }
}
