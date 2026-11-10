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
    url: 'https://iii.dev/api/subscribe/confirm',
  },
  redirects: { confirmed: 'https://iii.dev/subscribed', expired: 'https://iii.dev/subscribe-expired' },
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
function fakeResend(
  contacts: Record<string, { unsubscribed: boolean }> = {},
  opts: { failSend?: boolean; failSegment?: boolean } = {},
) {
  const calls: Call[] = []
  const fetch = (async (url: string, init: RequestInit) => {
    const path = url.replace('https://api.resend.com', '')
    const body = init.body ? JSON.parse(init.body as string) : undefined
    calls.push({ method: init.method as string, path, body, headers: init.headers as Record<string, string> })
    const reply = (status: number, data = {}) => new Response(JSON.stringify(data), { status })

    if (/^\/contacts\/[^/]+\/segments\//.test(path)) return opts.failSegment ? reply(500) : reply(200, { id: 'seg' })
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

function click(token: string | undefined): Request {
  return { method: 'GET', headers: {}, query_params: token === undefined ? {} : { token }, body: null }
}

/** The token of the link in the last confirmation email the fake sent. */
function sentToken(calls: Call[]): string {
  const send = calls.filter((c) => c.path === '/emails').at(-1)
  const link = (send?.body?.template as { variables: { CONFIRM_URL: string } }).variables.CONFIRM_URL
  return new URL(link).searchParams.get('token') as string
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

test('subscribe: a new address is sent a confirmation link, and nothing is stored', async () => {
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
  expect(template.variables.CONFIRM_URL).toStartWith('https://iii.dev/api/subscribe/confirm?token=')
  expect(send.headers.authorization).toBe('Bearer re_test')
})

test('confirm: a valid link sends the welcome, then creates the contact in the source segment', async () => {
  const resend = fakeResend()
  const { subscribe, confirm } = createSubscribe(config, resend.fetch)
  await subscribe(post({ email: 'ada@example.com', source: 'ade' }))
  const token = sentToken(resend.calls)

  resend.calls.length = 0
  const res = await confirm(click(token))
  expect(res.status_code).toBe(302)
  expect(res.headers.location).toBe('https://iii.dev/subscribed')
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
    await confirm(click(sentToken(resend.calls)))
    expect(resend.calls.at(-1)?.body?.segments).toEqual(segments)
  }
})

test('confirm: a bad, tampered, forged or expired link goes to the expired page and calls nothing', async () => {
  let t = 0
  const resend = fakeResend()
  const { subscribe, confirm } = createSubscribe(config, resend.fetch, () => t)
  await subscribe(post({ email: 'ada@example.com' }))
  const token = sentToken(resend.calls)
  const [payload, sig] = token.split('.')

  const otherKey = fakeResend()
  const forger = createSubscribe({ ...config, apiKey: 're_other' }, otherKey.fetch, () => t)
  await forger.subscribe(post({ email: 'ada@example.com' }))

  const swapped = Buffer.from(JSON.stringify({ e: 'eve@example.com', s: 'website', x: 9e15 })).toString('base64url')
  const bad = [
    undefined,
    '',
    'nonsense',
    `${payload}.${sig}.x`,
    `${swapped}.${sig}`,
    sentToken(otherKey.calls),
    'a'.repeat(5000),
  ]
  resend.calls.length = 0
  for (const candidate of bad) {
    const res = await confirm(click(candidate))
    expect(res.headers.location).toBe('https://iii.dev/subscribe-expired')
  }

  t = 3 * DAY + 1
  expect((await confirm(click(token))).headers.location).toBe('https://iii.dev/subscribe-expired')
  expect(resend.calls.length).toBe(0)
})

test('confirm: clicking twice sends one welcome', async () => {
  const contacts: Record<string, { unsubscribed: boolean }> = {}
  const resend = fakeResend(contacts)
  const { subscribe, confirm } = createSubscribe(config, resend.fetch)
  await subscribe(post({ email: 'ada@example.com' }))
  const token = sentToken(resend.calls)

  await confirm(click(token))
  contacts['ada@example.com'] = { unsubscribed: false }
  resend.calls.length = 0
  const again = await confirm(click(token))
  expect(again.headers.location).toBe('https://iii.dev/subscribed')
  expect(routes(resend.calls)).toEqual([
    'GET /contacts/ada%40example.com',
    'POST /contacts/ada%40example.com/segments/seg-website',
  ])
})

test('subscribe: an unsubscribed contact must confirm again; confirming re-subscribes without a welcome', async () => {
  const resend = fakeResend({ 'ada@example.com': { unsubscribed: true } })
  const { subscribe, confirm } = createSubscribe(config, resend.fetch)
  await subscribe(post({ email: 'ada@example.com' }))
  expect(routes(resend.calls)).toEqual(['GET /contacts/ada%40example.com', 'POST /emails'])
  const token = sentToken(resend.calls)

  resend.calls.length = 0
  await confirm(click(token))
  expect(routes(resend.calls)).toEqual([
    'GET /contacts/ada%40example.com',
    'PATCH /contacts/ada%40example.com',
    'POST /contacts/ada%40example.com/segments/seg-website',
  ])
  expect(resend.calls[1].body).toEqual({ unsubscribed: false })
})

test('subscribe: a subscribed address gets no email, only the segment of this source', async () => {
  const resend = fakeResend({ 'ada@example.com': { unsubscribed: false } })
  const { subscribe } = createSubscribe(config, resend.fetch)
  const res = await subscribe(post({ email: 'ada@example.com', source: 'ade' }))

  expect(res.status_code).toBe(200)
  expect(routes(resend.calls)).toEqual([
    'GET /contacts/ada%40example.com',
    'POST /contacts/ada%40example.com/segments/seg-ade',
  ])
})

test('a failed segment add for a known contact is logged, the answer is still a success', async () => {
  const resend = fakeResend({ 'ada@example.com': { unsubscribed: false } }, { failSegment: true })
  const { subscribe } = createSubscribe(config, resend.fetch)
  const res = await quietly(() => subscribe(post({ email: 'ada@example.com' })))
  expect(res.status_code).toBe(200)
})

test('subscribe: a raw JSON string body is parsed too', async () => {
  const resend = fakeResend()
  const { subscribe } = createSubscribe(config, resend.fetch)
  const res = await subscribe(post(JSON.stringify({ email: 'ada@example.com' })))
  expect(res.status_code).toBe(200)
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
    [post({ email: 'ada@example.com' }, 'https://evil.example'), 403],
    [{ ...post({ email: 'ada@example.com' }), method: 'GET' }, 405],
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

test('confirm: a failed welcome creates no contact, so a new link works once Resend does', async () => {
  const resend = fakeResend()
  const { subscribe, confirm } = createSubscribe(config, resend.fetch)
  await subscribe(post({ email: 'ada@example.com' }))
  const token = sentToken(resend.calls)

  resend.opts.failSend = true
  resend.calls.length = 0
  const failed = await quietly(() => confirm(click(token)))
  expect(failed.headers.location).toBe('https://iii.dev/subscribe-expired')
  expect(routes(resend.calls)).toEqual(['GET /contacts/ada%40example.com', 'POST /emails'])

  resend.opts.failSend = false
  resend.calls.length = 0
  const retried = await confirm(click(token))
  expect(retried.headers.location).toBe('https://iii.dev/subscribed')
  expect(routes(resend.calls)).toEqual(['GET /contacts/ada%40example.com', 'POST /emails', 'POST /contacts'])
})

test('the welcome idempotency key stays within 256 characters for the longest address', async () => {
  const resend = fakeResend()
  const { subscribe, confirm } = createSubscribe(config, resend.fetch)
  const email = `${'a'.repeat(64)}@${'b'.repeat(185)}.com` // 254 characters
  await subscribe(post({ email }))
  await confirm(click(sentToken(resend.calls)))
  const welcome = resend.calls.filter((c) => c.path === '/emails').at(-1)
  expect(welcome?.headers['idempotency-key'].length).toBeLessThanOrEqual(256)
})

test('confirm: only GET', async () => {
  const { confirm } = createSubscribe(config, fakeResend().fetch)
  expect((await confirm({ ...click('x'), method: 'POST' })).status_code).toBe(405)
})
