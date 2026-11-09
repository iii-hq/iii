// createSubscribe takes its config and fetch as arguments, so these tests drive
// it with a fake Resend that records every call.

import { expect, test } from 'bun:test'

import { type Config, createSubscribe, type Request } from './subscribe'

const config: Config = {
  apiKey: 're_test',
  templateId: 'tmpl-123',
  from: 'Hello from iii <hello@email.iii.dev>',
  subject: 'Welcome to the iii community',
  allowedOrigins: ['https://iii.dev', 'https://www.iii.dev'],
}

type Call = {
  method: string
  path: string
  body: Record<string, unknown> | undefined
  headers: Record<string, string>
}

// contacts: email -> { unsubscribed } for addresses Resend already knows.
function fakeResend(contacts: Record<string, { unsubscribed: boolean }> = {}, { failSend = false } = {}) {
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
    if (path === '/emails') return failSend ? reply(500) : reply(200, { id: 'e1' })
    return reply(404)
  }) as unknown as typeof globalThis.fetch
  return { fetch, calls }
}

function post(body: unknown, origin = 'https://iii.dev'): Request {
  return { method: 'POST', headers: { origin }, body }
}

test('new address: creates the contact and sends the welcome template once', async () => {
  const resend = fakeResend()
  const res = await createSubscribe(config, resend.fetch)(post({ email: '  Ada@Example.com ' }))

  expect(res.status_code).toBe(200)
  expect(resend.calls.map((c) => `${c.method} ${c.path}`)).toEqual([
    'GET /contacts/ada%40example.com',
    'POST /contacts',
    'POST /emails',
  ])
  expect(resend.calls[1].body).toEqual({ email: 'ada@example.com', unsubscribed: false })

  const send = resend.calls[2]
  expect(send.body).toEqual({
    from: config.from,
    to: 'ada@example.com',
    subject: config.subject,
    template: { id: 'tmpl-123' },
  })
  expect(send.headers['idempotency-key']).toBe('welcome/ada@example.com')
  expect(send.headers.authorization).toBe('Bearer re_test')
})

test('segment id, when configured, is attached to the new contact', async () => {
  const resend = fakeResend()
  await createSubscribe({ ...config, segmentId: 'seg-1' }, resend.fetch)(post({ email: 'ada@example.com' }))
  expect(resend.calls[1].body?.segments).toEqual([{ id: 'seg-1' }])
})

test('known address: no second welcome', async () => {
  const resend = fakeResend({ 'ada@example.com': { unsubscribed: false } })
  const res = await createSubscribe(config, resend.fetch)(post({ email: 'ada@example.com' }))

  expect(res.status_code).toBe(200)
  expect(resend.calls.map((c) => c.method)).toEqual(['GET'])
})

test('unsubscribed address that signs up again is re-subscribed, still no welcome', async () => {
  const resend = fakeResend({ 'ada@example.com': { unsubscribed: true } })
  const res = await createSubscribe(config, resend.fetch)(post({ email: 'ada@example.com' }))

  expect(res.status_code).toBe(200)
  expect(resend.calls.map((c) => `${c.method} ${c.path}`)).toEqual([
    'GET /contacts/ada%40example.com',
    'PATCH /contacts/ada%40example.com',
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
    [post({ email: 'ada@example.com' }, 'https://evil.example'), 403],
    [{ ...post({ email: 'ada@example.com' }), method: 'GET' }, 405],
  ]
  for (const [req, status] of cases) expect((await subscribe(req)).status_code).toBe(status)
  expect(resend.calls.length).toBe(0)
})

test('Resend failure surfaces as 502', async () => {
  const resend = fakeResend({}, { failSend: true })
  const original = console.error
  console.error = () => {
    // the handler logs the Resend failure; keep the test output clean
  }
  try {
    const res = await createSubscribe(config, resend.fetch)(post({ email: 'ada@example.com' }))
    expect(res.status_code).toBe(502)
  } finally {
    console.error = original
  }
})
