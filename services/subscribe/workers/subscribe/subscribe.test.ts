// createSubscribe takes its config, fetch and clock as arguments, so these
// tests drive it with a fake Resend that records every call.

import { expect, test } from 'bun:test'
import { createHash } from 'node:crypto'

import { type Config, createSubscribe, type Request } from './subscribe'

const DAY = 86_400_000

const config: Config = {
  apiKey: 're_test',
  from: 'Hello from iii <hello@email.iii.dev>',
  welcome: { templateId: 'tmpl-welcome', subject: 'Welcome to the iii community' },
  confirmation: {
    templateId: 'confirm-subscription',
    subject: 'Confirm your iii subscription',
    url: 'https://iii.dev/subscribe-confirm',
  },
  segments: { website: 'seg-website', ade: 'seg-ade', untracked: '' },
  defaultSource: 'website',
  allowedOrigins: ['https://iii.dev', 'https://www.iii.dev'],
}

type Call = {
  method: string
  path: string
  body: Record<string, unknown> | undefined
  headers: Record<string, string>
}

// contacts: email -> { unsubscribed } for addresses Resend already knows.
function fakeResend(contacts: Record<string, { unsubscribed: boolean }> = {}, opts: { failSend?: boolean } = {}) {
  const calls: Call[] = []
  const fetch = (async (url: string, init: RequestInit) => {
    const path = url.replace('https://api.resend.com', '')
    const body = init.body ? JSON.parse(init.body as string) : undefined
    calls.push({ method: init.method as string, path, body, headers: init.headers as Record<string, string> })
    const reply = (status: number, data = {}) => new Response(JSON.stringify(data), { status })

    if (init.method === 'GET' && path.startsWith('/contacts/')) {
      const contact = contacts[decodeURIComponent(path.slice('/contacts/'.length))]
      return contact ? reply(200, { object: 'contact', ...contact }) : reply(404, { name: 'not_found' })
    }
    if (path.startsWith('/contacts')) return reply(200, { object: 'contact', id: 'c1' })
    if (path === '/emails') return opts.failSend ? reply(500) : reply(200, { id: 'e1' })
    return reply(404)
  }) as unknown as typeof globalThis.fetch
  return { fetch, calls, opts }
}

const routes = (calls: Call[]) => calls.map((c) => `${c.method} ${c.path}`)

function post(body: unknown, origin = 'https://iii.dev'): Request {
  return { method: 'POST', headers: { origin }, body }
}

/** The token of the link in the last confirmation email the fake sent. */
function sentToken(calls: Call[]): string {
  const send = calls.filter((c) => c.path === '/emails').at(-1)
  const link = (send?.body?.template as { variables: { CONFIRM_URL: string } }).variables.CONFIRM_URL
  return new URLSearchParams(new URL(link).hash.slice(1)).get('token') as string
}

function quietly<T>(run: () => Promise<T>): Promise<T> {
  const original = console.error
  console.error = () => {
    // the handler logs Resend failures; keep the test output clean
  }
  return run().finally(() => {
    console.error = original
  })
}

test('subscribe: a new address gets a confirmation link to the confirm page, and nothing is stored', async () => {
  const resend = fakeResend()
  const { subscribe } = createSubscribe(config, resend.fetch)
  const res = await subscribe(post({ email: '  Ada@Example.com ' }))

  expect(res.status_code).toBe(200)
  expect(routes(resend.calls)).toEqual(['GET /contacts/ada%40example.com', 'POST /emails'])
  const send = resend.calls[1]
  expect(send.body?.to).toBe('ada@example.com')
  expect(send.body?.subject).toBe('Confirm your iii subscription')
  const template = send.body?.template as { id: string; variables: { CONFIRM_URL: string } }
  expect(template.id).toBe('confirm-subscription')
  // The token rides after `#`, so it never reaches a server log.
  expect(template.variables.CONFIRM_URL).toStartWith('https://iii.dev/subscribe-confirm#token=')
  expect(send.headers.authorization).toBe('Bearer re_test')
})

test('the token hides the address and anything else it carries', async () => {
  const resend = fakeResend()
  const { subscribe } = createSubscribe(config, resend.fetch)
  await subscribe(post({ email: 'ada.lovelace@example.com', source: 'ade' }))
  const decoded = Buffer.from(sentToken(resend.calls), 'base64url').toString('latin1')
  for (const secret of ['ada', 'lovelace', 'example', 'ade', 're_test']) expect(decoded).not.toContain(secret)
})

test('confirm: a valid token sends the welcome, then creates the contact in the source segment', async () => {
  const resend = fakeResend()
  const { subscribe, confirm } = createSubscribe(config, resend.fetch)
  await subscribe(post({ email: 'ada@example.com', source: 'ade' }))
  const token = sentToken(resend.calls)

  resend.calls.length = 0
  const res = await confirm(post({ token }))
  expect(res.status_code).toBe(200)
  expect(res.body).toEqual({ ok: true })
  expect(routes(resend.calls)).toEqual(['GET /contacts/ada%40example.com', 'POST /emails', 'POST /contacts'])

  const welcome = resend.calls[1]
  expect(welcome.body).toEqual({
    from: config.from,
    to: 'ada@example.com',
    subject: 'Welcome to the iii community',
    template: { id: 'tmpl-welcome' },
  })
  const digest = createHash('sha256').update('ada@example.com').digest('hex')
  expect(welcome.headers['idempotency-key']).toBe(`welcome/${digest}`)
  expect(resend.calls[2].body).toEqual({ email: 'ada@example.com', unsubscribed: false, segments: [{ id: 'seg-ade' }] })
})

test('confirm: website is the default source, and a source with no segment joins none', async () => {
  for (const [source, segments] of [
    [undefined, [{ id: 'seg-website' }]],
    ['untracked', undefined],
  ] as const) {
    const resend = fakeResend()
    const { subscribe, confirm } = createSubscribe(config, resend.fetch)
    await subscribe(post({ email: 'ada@example.com', source }))
    await confirm(post({ token: sentToken(resend.calls) }))
    expect(resend.calls.at(-1)?.body?.segments).toEqual(segments)
  }
})

test('confirm: a bad, altered, foreign or expired token is a 400 and calls nothing', async () => {
  let t = 0
  const resend = fakeResend()
  const { subscribe, confirm } = createSubscribe(config, resend.fetch, () => t)
  await subscribe(post({ email: 'ada@example.com' }))
  const token = sentToken(resend.calls)

  const otherKey = fakeResend()
  const other = createSubscribe({ ...config, apiKey: 're_other' }, otherKey.fetch, () => t)
  await other.subscribe(post({ email: 'ada@example.com' }))

  const flipped = Buffer.from(token, 'base64url')
  flipped[20] ^= 1
  const bad = [
    undefined,
    '',
    42,
    'nonsense',
    'a.b',
    `${token}=`,
    token.slice(0, -2),
    flipped.toString('base64url'),
    sentToken(otherKey.calls),
    'a'.repeat(5000),
  ]
  resend.calls.length = 0
  for (const candidate of bad) {
    const res = await confirm(post({ token: candidate }))
    expect(res.status_code).toBe(400)
    expect(res.body).toEqual({ error: 'invalid_link' })
  }

  t = 3 * DAY + 1
  expect((await confirm(post({ token }))).status_code).toBe(400)
  expect(resend.calls.length).toBe(0)
})

test('confirm: confirming twice sends one welcome', async () => {
  const contacts: Record<string, { unsubscribed: boolean }> = {}
  const resend = fakeResend(contacts)
  const { subscribe, confirm } = createSubscribe(config, resend.fetch)
  await subscribe(post({ email: 'ada@example.com' }))
  const token = sentToken(resend.calls)

  await confirm(post({ token }))
  contacts['ada@example.com'] = { unsubscribed: false }
  resend.calls.length = 0
  const again = await confirm(post({ token }))
  expect(again.status_code).toBe(200)
  expect(routes(resend.calls)).toEqual(['GET /contacts/ada%40example.com'])
})

test('confirm: the link of a first signup cannot re-subscribe someone who unsubscribed since', async () => {
  const contacts: Record<string, { unsubscribed: boolean }> = {}
  const resend = fakeResend(contacts)
  const { subscribe, confirm } = createSubscribe(config, resend.fetch)
  await subscribe(post({ email: 'ada@example.com' }))
  const firstLink = sentToken(resend.calls)
  await confirm(post({ token: firstLink }))

  contacts['ada@example.com'] = { unsubscribed: true }
  resend.calls.length = 0
  const replay = await confirm(post({ token: firstLink }))
  expect(replay.status_code).toBe(400)
  expect(replay.body).toEqual({ error: 'invalid_link' })
  expect(routes(resend.calls)).toEqual(['GET /contacts/ada%40example.com'])
})

test('an unsubscribed address that signs up again confirms with its new link, with no second welcome', async () => {
  const resend = fakeResend({ 'ada@example.com': { unsubscribed: true } })
  const { subscribe, confirm } = createSubscribe(config, resend.fetch)
  await subscribe(post({ email: 'ada@example.com' }))
  expect(routes(resend.calls)).toEqual(['GET /contacts/ada%40example.com', 'POST /emails'])
  const token = sentToken(resend.calls)

  resend.calls.length = 0
  const res = await confirm(post({ token }))
  expect(res.status_code).toBe(200)
  expect(routes(resend.calls)).toEqual(['GET /contacts/ada%40example.com', 'PATCH /contacts/ada%40example.com'])
  expect(resend.calls[1].body).toEqual({ unsubscribed: false })
})

test('subscribe: a subscribed address gets nothing and joins no segment, whatever the source', async () => {
  const resend = fakeResend({ 'ada@example.com': { unsubscribed: false } })
  const { subscribe } = createSubscribe(config, resend.fetch)
  const res = await subscribe(post({ email: 'ada@example.com', source: 'ade' }))

  expect(res.status_code).toBe(200)
  expect(res.body).toEqual({ ok: true })
  expect(routes(resend.calls)).toEqual(['GET /contacts/ada%40example.com'])
})

test('subscribe: a raw JSON string body is parsed too', async () => {
  const resend = fakeResend()
  const { subscribe } = createSubscribe(config, resend.fetch)
  const res = await subscribe(post(JSON.stringify({ email: 'ada@example.com' })))
  expect(res.status_code).toBe(200)
})

function serverPost(body: unknown): Request {
  return { method: 'POST', headers: {}, body }
}

test('ade: a server-to-server signup with no Origin follows the usual flow into the ade segment', async () => {
  const resend = fakeResend()
  const { subscribe, confirm } = createSubscribe(config, resend.fetch)
  const res = await subscribe(serverPost({ email: 'ada@example.com', source: 'ade' }))

  expect(res.status_code).toBe(200)
  expect(routes(resend.calls)).toEqual(['GET /contacts/ada%40example.com', 'POST /emails'])

  // The emailed link is confirmed on iii.dev, in a browser.
  await confirm(post({ token: sentToken(resend.calls) }))
  expect(resend.calls.at(-1)?.body).toEqual({
    email: 'ada@example.com',
    unsubscribed: false,
    segments: [{ id: 'seg-ade' }],
  })
})

test('ade: Origin rules on subscribe', async () => {
  const resend = fakeResend()
  const { subscribe } = createSubscribe(config, resend.fetch)
  const body = { email: 'ada@example.com', source: 'ade' }

  expect((await subscribe(serverPost(body))).status_code).toBe(200)
  expect((await subscribe(post(body, 'https://iii.dev'))).status_code).toBe(200)
  // Present but not allowed: still refused, including the opaque `null` origin and an empty one.
  for (const origin of ['https://evil.example', 'null', '', 'http://localhost:3000']) {
    expect((await subscribe(post(body, origin))).status_code).toBe(403)
  }
  // Origin is matched whatever its header's case.
  expect((await subscribe({ method: 'POST', headers: { Origin: 'https://evil.example' }, body })).status_code).toBe(403)
})

test('ade: an invalid email or an unknown source with no Origin is a 400 before Resend', async () => {
  const resend = fakeResend()
  const { subscribe } = createSubscribe(config, resend.fetch)
  expect((await subscribe(serverPost({ email: 'not-an-email', source: 'ade' }))).body).toEqual({
    error: 'invalid_email',
  })
  expect((await subscribe(serverPost({ email: 'ada@example.com', source: 'invalid' }))).body).toEqual({
    error: 'invalid_source',
  })
  expect(resend.calls.length).toBe(0)
})

test('confirm still needs an allowed Origin: a missing one is refused', async () => {
  const resend = fakeResend()
  const { subscribe, confirm } = createSubscribe(config, resend.fetch)
  await subscribe(post({ email: 'ada@example.com' }))
  const token = sentToken(resend.calls)

  resend.calls.length = 0
  expect((await confirm(serverPost({ token }))).status_code).toBe(403)
  expect(resend.calls.length).toBe(0)
})

test('the per-minute cap applies to calls with no Origin too', async () => {
  const resend = fakeResend()
  const { subscribe } = createSubscribe({ ...config, maxEmailsPerMinute: 1 }, resend.fetch, () => 0)
  expect((await subscribe(serverPost({ email: 'a@example.com', source: 'ade' }))).status_code).toBe(200)
  expect((await subscribe(serverPost({ email: 'b@example.com', source: 'ade' }))).status_code).toBe(429)
})

test('both routes refuse other methods and origins before Resend', async () => {
  const resend = fakeResend()
  const { subscribe, confirm } = createSubscribe(config, resend.fetch)
  for (const handler of [subscribe, confirm]) {
    expect((await handler(post({ email: 'ada@example.com' }, 'https://evil.example'))).status_code).toBe(403)
    expect((await handler({ ...post({ email: 'ada@example.com' }), method: 'GET' })).status_code).toBe(405)
  }
  expect(resend.calls.length).toBe(0)
})

test('subscribe: rejects bad input without calling Resend', async () => {
  const resend = fakeResend()
  const { subscribe } = createSubscribe(config, resend.fetch)
  const cases: [Request, number][] = [
    [post({ email: 'not-an-email' }), 400],
    [post('{not json'), 400],
    [post(null), 400],
    [post({ email: `${'a'.repeat(250)}@example.com` }), 400],
    [post({ email: 'ada@example.com', pad: 'x'.repeat(2000) }), 400],
    // 400 three-byte characters: under 1024 characters, over 1024 bytes.
    [post({ email: 'ada@example.com', pad: '€'.repeat(400) }), 400],
    [post({ email: 'ada@example.com', source: 'nope' }), 400],
    [post({ email: 'ada@example.com', source: 'seg-website' }), 400],
    [post({ email: 'ada@example.com', source: '__proto__' }), 400],
    [post({ email: 'ada@example.com', source: 42 }), 400],
  ]
  for (const [req, status] of cases) expect((await subscribe(req)).status_code).toBe(status)
  expect(resend.calls.length).toBe(0)
})

test('subscribe: confirmation emails past the per-minute cap get a 429 and no email', async () => {
  const resend = fakeResend()
  let t = 0
  const { subscribe } = createSubscribe({ ...config, maxEmailsPerMinute: 2 }, resend.fetch, () => t)

  const codes = []
  for (const name of ['a', 'b', 'c']) codes.push((await subscribe(post({ email: `${name}@example.com` }))).status_code)
  expect(codes).toEqual([200, 200, 429])
  expect(resend.calls.filter((c) => c.path === '/emails').length).toBe(2)

  t += 60_000
  expect((await subscribe(post({ email: 'c@example.com' }))).status_code).toBe(200)
})

test('subscribe: a Resend failure surfaces as 502', async () => {
  const resend = fakeResend({}, { failSend: true })
  const { subscribe } = createSubscribe(config, resend.fetch)
  const res = await quietly(() => subscribe(post({ email: 'ada@example.com' })))
  expect(res.status_code).toBe(502)
})

test('confirm: a failed welcome is a 502 and creates no contact, so confirming again works', async () => {
  const resend = fakeResend()
  const { subscribe, confirm } = createSubscribe(config, resend.fetch)
  await subscribe(post({ email: 'ada@example.com' }))
  const token = sentToken(resend.calls)

  resend.opts.failSend = true
  resend.calls.length = 0
  const failed = await quietly(() => confirm(post({ token })))
  expect(failed.status_code).toBe(502)
  expect(routes(resend.calls)).toEqual(['GET /contacts/ada%40example.com', 'POST /emails'])

  resend.opts.failSend = false
  resend.calls.length = 0
  const retried = await confirm(post({ token }))
  expect(retried.status_code).toBe(200)
  expect(routes(resend.calls)).toEqual(['GET /contacts/ada%40example.com', 'POST /emails', 'POST /contacts'])
})

test('the welcome idempotency key stays within 256 characters for the longest address', async () => {
  const resend = fakeResend()
  const { subscribe, confirm } = createSubscribe(config, resend.fetch)
  const email = `${'a'.repeat(64)}@${'b'.repeat(185)}.com` // 254 characters
  await subscribe(post({ email }))
  await confirm(post({ token: sentToken(resend.calls) }))
  const welcome = resend.calls.filter((c) => c.path === '/emails').at(-1)
  expect(welcome?.headers['idempotency-key'].length).toBeLessThanOrEqual(256)
})
