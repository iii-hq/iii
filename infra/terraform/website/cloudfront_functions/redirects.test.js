// redirects.js is authored for the cloudfront-js-2.0 runtime and has no CJS/ESM
// wrapper, so we load it via `new Function(...)` rather than `require`.

const test = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')

const source = fs.readFileSync(path.join(__dirname, 'redirects.js'), 'utf8')
const handler = new Function(source + '\nreturn handler;')()

function buildEvent(uri, host) {
  return {
    version: '1.0',
    context: {},
    viewer: {},
    request: {
      method: 'GET',
      uri: uri,
      querystring: {},
      headers: host ? { host: { value: host } } : {},
      cookies: {},
    },
  }
}

function isRedirect(result) {
  return (
    result &&
    typeof result === 'object' &&
    result.statusCode === 301 &&
    result.headers &&
    result.headers.location &&
    typeof result.headers.location.value === 'string'
  )
}

function locationOf(result) {
  return result.headers.location.value
}

test('/docs exact → 301 https://docs.iii.dev/docs', () => {
  const result = handler(buildEvent('/docs', 'iii.dev'))
  assert.ok(isRedirect(result))
  assert.equal(locationOf(result), 'https://docs.iii.dev/docs')
})

test('/docs/ trailing slash → 301 https://docs.iii.dev/docs/', () => {
  const result = handler(buildEvent('/docs/', 'iii.dev'))
  assert.ok(isRedirect(result))
  assert.equal(locationOf(result), 'https://docs.iii.dev/docs/')
})

test('/docs/quickstart → 301 https://docs.iii.dev/docs/quickstart', () => {
  const result = handler(buildEvent('/docs/quickstart', 'iii.dev'))
  assert.ok(isRedirect(result))
  assert.equal(locationOf(result), 'https://docs.iii.dev/docs/quickstart')
})

test('/docs/guide/deep/nested → preserves deep path on redirect', () => {
  const result = handler(buildEvent('/docs/guide/deep/nested', 'iii.dev'))
  assert.ok(isRedirect(result))
  assert.equal(locationOf(result), 'https://docs.iii.dev/docs/guide/deep/nested')
})

test('/docsfoo → NOT redirected (not under /docs/)', () => {
  const result = handler(buildEvent('/docsfoo', 'iii.dev'))
  assert.ok(!isRedirect(result), 'should not be a redirect')
  assert.equal(result.uri, '/index.html')
})

test('/llms.txt → pass through unchanged (matches current 404 behavior)', () => {
  const result = handler(buildEvent('/llms.txt', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/llms.txt')
})

test('www.iii.dev/ → 301 https://iii.dev/', () => {
  const result = handler(buildEvent('/', 'www.iii.dev'))
  assert.ok(isRedirect(result))
  assert.equal(locationOf(result), 'https://iii.dev/')
})

test('www.iii.dev/some/page → 301 https://iii.dev/some/page', () => {
  const result = handler(buildEvent('/some/page', 'www.iii.dev'))
  assert.ok(isRedirect(result))
  assert.equal(locationOf(result), 'https://iii.dev/some/page')
})

test('www.iii.dev/docs/foo → 301 https://docs.iii.dev/docs/foo (ONE hop, not two)', () => {
  const result = handler(buildEvent('/docs/foo', 'www.iii.dev'))
  assert.ok(isRedirect(result))
  assert.equal(
    locationOf(result),
    'https://docs.iii.dev/docs/foo',
    'docs redirect must win over www→apex redirect to avoid a 2-hop chain',
  )
})

test('/ (root) → pass through unchanged', () => {
  const result = handler(buildEvent('/', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/')
})

test('/some/client/route → rewrite uri to /index.html', () => {
  const result = handler(buildEvent('/some/client/route', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/index.html')
})

test('/manifesto → rewrite uri to /index.html', () => {
  const result = handler(buildEvent('/manifesto', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/index.html')
})

test('/foo/ trailing slash → pass through unchanged (no SPA rewrite)', () => {
  const result = handler(buildEvent('/foo/', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/foo/')
})

test('/missing.jpg → pass through unchanged (S3 returns 404)', () => {
  const result = handler(buildEvent('/missing.jpg', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/missing.jpg')
})

test('/ai/index.html → pass through unchanged', () => {
  const result = handler(buildEvent('/ai/index.html', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/ai/index.html')
})

test('/assets/main.abc123.js → pass through unchanged', () => {
  const result = handler(buildEvent('/assets/main.abc123.js', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/assets/main.abc123.js')
})

test('/favicon.svg → pass through unchanged', () => {
  const result = handler(buildEvent('/favicon.svg', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/favicon.svg')
})

test('/.well-known/vercel/project.json → pass through (no SPA rewrite)', () => {
  const result = handler(buildEvent('/.well-known/vercel/project.json', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/.well-known/vercel/project.json')
})

test('/.well-known/foo (no extension) → pass through, NOT SPA rewritten', () => {
  const result = handler(buildEvent('/.well-known/foo', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/.well-known/foo', '.well-known is an explicit exemption from SPA fallback')
})

test('missing host header → still handles other rules correctly', () => {
  const event = buildEvent('/docs/foo', undefined)
  delete event.request.headers.host
  const result = handler(event)
  assert.ok(isRedirect(result))
  assert.equal(locationOf(result), 'https://docs.iii.dev/docs/foo')
})
