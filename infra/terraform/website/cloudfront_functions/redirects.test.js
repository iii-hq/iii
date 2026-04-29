// redirects.js is authored for the cloudfront-js-2.0 runtime and has no CJS/ESM
// wrapper, so we load it via `new Function(...)` rather than `require`.

const test = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')

const source = fs.readFileSync(path.join(__dirname, 'redirects.js'), 'utf8')
const handler = new Function(source + '\nreturn handler;')()

function buildEvent(uri, host, querystring) {
  return {
    version: '1.0',
    context: {},
    viewer: {},
    request: {
      method: 'GET',
      uri: uri,
      querystring: querystring || {},
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

test('/docs → function does not redirect', () => {
  const result = handler(buildEvent('/docs', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/index.html')
})

test('/docs/quickstart → function does not redirect', () => {
  const result = handler(buildEvent('/docs/quickstart', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/index.html')
})

test('/docsfoo → NOT redirected (not under /docs/)', () => {
  const result = handler(buildEvent('/docsfoo', 'iii.dev'))
  assert.ok(!isRedirect(result), 'should not be a redirect')
  assert.equal(result.uri, '/index.html')
})

test('/llms.txt → pass through unchanged (static file)', () => {
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

test('www.iii.dev/docs/foo → 301 https://iii.dev/docs/foo', () => {
  const result = handler(buildEvent('/docs/foo', 'www.iii.dev'))
  assert.ok(isRedirect(result))
  assert.equal(locationOf(result), 'https://iii.dev/docs/foo')
})

test('www.iii.dev preserves querystring with multiValue and empty params', () => {
  // Mirrors the CloudFront Functions querystring shape: repeated keys spill into
  // multiValue, value-less keys arrive as empty strings, and special chars must
  // be re-encoded.
  const result = handler(
    buildEvent('/some/page', 'www.iii.dev', {
      a: { value: '1', multiValue: [{ value: '2' }] },
      empty: { value: '' },
      ref: { value: 'hello world' },
    }),
  )
  assert.ok(isRedirect(result))
  assert.equal(
    locationOf(result),
    'https://iii.dev/some/page?a=1&a=2&empty=&ref=hello%20world',
  )
})

test('www.iii.dev with no querystring → no trailing ?', () => {
  const result = handler(buildEvent('/some/page', 'www.iii.dev', {}))
  assert.ok(isRedirect(result))
  assert.equal(locationOf(result), 'https://iii.dev/some/page')
})

test('www.iii.dev percent-encodes reserved chars in keys and values', () => {
  // Values containing &, =, #, + would otherwise corrupt the redirect target
  // (& splits params, # ends the URL into a fragment, + flips to space on parse,
  // = confuses some clients). Keys with spaces must also be encoded.
  const result = handler(
    buildEvent('/p', 'www.iii.dev', {
      'weird key': { value: 'a&b=c+d#e' },
    }),
  )
  assert.ok(isRedirect(result))
  assert.equal(
    locationOf(result),
    'https://iii.dev/p?weird%20key=a%26b%3Dc%2Bd%23e',
  )
})

test('SPA fallback preserves querystring on the request object (no rewrite)', () => {
  // The handler mutates request.uri but returns the same request object, so
  // CloudFront forwards the original querystring untouched. Pin the no-op so
  // a future refactor doesn't accidentally clear it.
  const qs = { utm_source: { value: 'twitter' }, ref: { value: 'launch' } }
  const event = buildEvent('/some/route', 'iii.dev', qs)
  const result = handler(event)
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/index.html')
  assert.equal(result.querystring, qs)
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

test('/manifesto → rewrite uri to /manifesto.html (flat HTML, Option A)', () => {
  const result = handler(buildEvent('/manifesto', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/manifesto.html')
})

test('/AGENTS.md → pass through unchanged', () => {
  const result = handler(buildEvent('/AGENTS.md', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/AGENTS.md')
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

test('/ai → SPA fallback to /index.html', () => {
  const result = handler(buildEvent('/ai', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/index.html')
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

test('missing host header → SPA fallback still applies for extensionless paths', () => {
  const event = buildEvent('/some/page', undefined)
  delete event.request.headers.host
  const result = handler(event)
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/index.html')
})
