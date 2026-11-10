// createSubscribe takes its config and fetch as arguments, so these tests drive
// it with a fake Resend that records every call.

import { expect, test } from 'bun:test'
import { createHash } from 'node:crypto'

import { type Config, createSubscribe, type Request } from './subscribe'

const config: Config = {
  apiKey: 're_test',
  templateId: 'tmpl-123',
  from: 'Hello from iii <hello@email.iii.dev>',
  subject: 'Welcome to the iii community',
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
  opts: { failSend: boolean; failSegment?: boolean } = { failSend: false },
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

function post(body: unknown, origin = 'https://iii.dev'): Request {
  return { method: 'POST', headers: { origin }, body }
}

test('new address: sends the welcome template, then creates the contact', async () => {
  const resend = fakeResend()
  const res = await createSubscribe(config, resend.fetch)(post({ email: '  Ada@Example.com ' }))

  expect(res.status_code).toBe(200)
  expect(resend.calls.map((c) => `${c.method} ${c.path}`)).toEqual([
    'GET /contacts/ada%40example.com',
    'POST /emails',
    'POST /contacts',
  ])
  expect(resend.calls[2].body).toEqual({
    email: 'ada@example.com',
    unsubscribed: false,
    segments: [{ id: 'seg-website' }],
  })

  const send = resend.calls[1]
  expect(send.body).toEqual({
    from: config.from,
    to: 'ada@example.com',
    subject: config.subject,
    template: { id: 'tmpl-123' },
  })
  const digest = createHash('sha256').update('ada@example.com').digest('hex')
  expect(send.headers['idempotency-key']).toBe(`welcome/${digest}`)
  expect(send.headers.authorization).toBe('Bearer re_test')
})

test('a new contact joins the segment of its source, website by default', async () => {
  const website = fakeResend()
  await createSubscribe(config, website.fetch)(post({ email: 'ada@example.com' }))
  expect(website.calls[2].body?.segments).toEqual([{ id: 'seg-website' }])

  const ade = fakeResend()
  await createSubscribe(config, ade.fetch)(post({ email: 'ada@example.com', source: 'ade' }))
  expect(ade.calls[2].body?.segments).toEqual([{ id: 'seg-ade' }])

  const untracked = fakeResend()
  await createSubscribe(config, untracked.fetch)(post({ email: 'ada@example.com', source: 'untracked' }))
  expect(untracked.calls[2].body?.segments).toBeUndefined()
})

test('a source that is not configured is refused before Resend', async () => {
  const resend = fakeResend()
  const subscribe = createSubscribe(config, resend.fetch)
  for (const source of ['nope', 'seg-website', 42, '__proto__', 'toString']) {
    expect((await subscribe(post({ email: 'ada@example.com', source }))).status_code).toBe(400)
  }
  expect(resend.calls.length).toBe(0)
})

test('known address: no second welcome, and it joins the segment of this source', async () => {
  const resend = fakeResend({ 'ada@example.com': { unsubscribed: false } })
  const res = await createSubscribe(config, resend.fetch)(post({ email: 'ada@example.com', source: 'ade' }))

  expect(res.status_code).toBe(200)
  expect(resend.calls.map((c) => `${c.method} ${c.path}`)).toEqual([
    'GET /contacts/ada%40example.com',
    'POST /contacts/ada%40example.com/segments/seg-ade',
  ])
})

test('a failed segment add for a known address is logged, the signup still answers 200', async () => {
  const resend = fakeResend({ 'ada@example.com': { unsubscribed: false } }, { failSend: false, failSegment: true })
  const res = await quietly(() => createSubscribe(config, resend.fetch)(post({ email: 'ada@example.com' })))
  expect(res.status_code).toBe(200)
})

test('unsubscribed address that signs up again is re-subscribed, still no welcome', async () => {
  const resend = fakeResend({ 'ada@example.com': { unsubscribed: true } })
  const res = await createSubscribe(config, resend.fetch)(post({ email: 'ada@example.com' }))

  expect(res.status_code).toBe(200)
  expect(resend.calls.map((c) => `${c.method} ${c.path}`)).toEqual([
    'GET /contacts/ada%40example.com',
    'PATCH /contacts/ada%40example.com',
    'POST /contacts/ada%40example.com/segments/seg-website',
  ])
  expect(resend.calls[1].body).toEqual({ unsubscribed: false })
})

test('a raw JSON string body is parsed too', async () => {
  const resend = fakeResend()
  const res = await createSubscribe(config, resend.fetch)(post(JSON.stringify({ email: 'ada@example.com' })))
  expect(res.status_code).toBe(200)
})

test('rejects bad input without calling Resend', async () => {
  const resend = fakeResend()
  const subscribe = createSubscribe(config, resend.fetch)
  const cases: [Request, number][] = [
    [post({ email: 'not-an-email' }), 400],
    [post('{not json'), 400],
    [post(null), 400],
    [post({ email: `${'a'.repeat(250)}@example.com` }), 400],
    [post({ email: 'ada@example.com', pad: 'x'.repeat(2000) }), 400],
    // 400 three-byte characters: under 1024 characters, over 1024 bytes.
    [post({ email: 'ada@example.com', pad: '€'.repeat(400) }), 400],
    [post({ email: 'ada@example.com' }, 'https://evil.example'), 403],
    [{ ...post({ email: 'ada@example.com' }), method: 'GET' }, 405],
  ]
  for (const [req, status] of cases) expect((await subscribe(req)).status_code).toBe(status)
  expect(resend.calls.length).toBe(0)
})

function quietly<T>(run: () => Promise<T>): Promise<T> {
  const original = console.error
  console.error = () => {
    // the handler logs the Resend failure; keep the test output clean
  }
  return run().finally(() => {
    console.error = original
  })
}

test('a failed welcome creates no contact, so the next submit sends it', async () => {
  const resend = fakeResend({}, { failSend: true })
  const subscribe = createSubscribe(config, resend.fetch)

  const failed = await quietly(() => subscribe(post({ email: 'ada@example.com' })))
  expect(failed.status_code).toBe(502)
  expect(resend.calls.map((c) => `${c.method} ${c.path}`)).toEqual(['GET /contacts/ada%40example.com', 'POST /emails'])

  resend.opts.failSend = false
  resend.calls.length = 0
  const retried = await subscribe(post({ email: 'ada@example.com' }))
  expect(retried.status_code).toBe(200)
  expect(resend.calls.map((c) => `${c.method} ${c.path}`)).toEqual([
    'GET /contacts/ada%40example.com',
    'POST /emails',
    'POST /contacts',
  ])
})

test('the idempotency key stays within 256 characters for the longest address', async () => {
  const resend = fakeResend()
  const email = `${'a'.repeat(64)}@${'b'.repeat(185)}.com` // 254 characters
  await createSubscribe(config, resend.fetch)(post({ email }))
  expect(resend.calls[1].headers['idempotency-key'].length).toBeLessThanOrEqual(256)
})

test('welcomes past the per-minute cap get a 429 and no email', async () => {
  const resend = fakeResend()
  let t = 0
  const subscribe = createSubscribe({ ...config, maxWelcomesPerMinute: 2 }, resend.fetch, () => t)

  const codes = []
  for (const name of ['a', 'b', 'c']) codes.push((await subscribe(post({ email: `${name}@example.com` }))).status_code)
  expect(codes).toEqual([200, 200, 429])
  expect(resend.calls.filter((c) => c.path === '/emails').length).toBe(2)

  t += 60_000
  expect((await subscribe(post({ email: 'c@example.com' }))).status_code).toBe(200)
})

test('Resend failure surfaces as 502', async () => {
  const resend = fakeResend({}, { failSend: true })
  const res = await quietly(() => createSubscribe(config, resend.fetch)(post({ email: 'ada@example.com' })))
  expect(res.status_code).toBe(502)
})
