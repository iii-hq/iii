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
  assert.equal(locationOf(result), 'https://iii.dev/some/page?a=1&a=2&empty=&ref=hello%20world')
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
  assert.equal(locationOf(result), 'https://iii.dev/p?weird%20key=a%26b%3Dc%2Bd%23e')
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

test('/manifesto.html → pass through unchanged', () => {
  const result = handler(buildEvent('/manifesto.html', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/manifesto.html')
})

test('/manifesto/ trailing slash → pass through (pretty-URL rewrite only matches exact extensionless path)', () => {
  const result = handler(buildEvent('/manifesto/', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/manifesto/')
})

test('www.iii.dev/manifesto → 301 https://iii.dev/manifesto (host-redirect runs before pretty-URL rewrite)', () => {
  const result = handler(buildEvent('/manifesto', 'www.iii.dev'))
  assert.ok(isRedirect(result))
  assert.equal(locationOf(result), 'https://iii.dev/manifesto')
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

// ---------------------------------------------------------------------------
// /blog/* — Astro blog at iii.dev/blog. Astro emits with trailingSlash: 'always'
// and build.format: 'directory', so each post is a key like
// blog/<slug>/index.html in S3. CloudFront's default_root_object only applies
// to the apex, so /blog/<slug>/ must be rewritten to .../index.html and
// extensionless paths must 301 to the canonical trailing-slash form.
// ---------------------------------------------------------------------------

test('/blog → 301 https://iii.dev/blog/ (canonical trailing slash)', () => {
  const result = handler(buildEvent('/blog', 'iii.dev'))
  assert.ok(isRedirect(result))
  assert.equal(locationOf(result), 'https://iii.dev/blog/')
})

test('/blog/ → rewrite to /blog/index.html', () => {
  const result = handler(buildEvent('/blog/', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/blog/index.html')
})

test('/blog/hello-world/ → rewrite to /blog/hello-world/index.html', () => {
  const result = handler(buildEvent('/blog/hello-world/', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/blog/hello-world/index.html')
})

test('/blog/hello-world (no trailing slash) → 301 to /blog/hello-world/', () => {
  const result = handler(buildEvent('/blog/hello-world', 'iii.dev'))
  assert.ok(isRedirect(result))
  assert.equal(locationOf(result), 'https://iii.dev/blog/hello-world/')
})

test('/blog/foo/bar/ → rewrite to /blog/foo/bar/index.html (nested)', () => {
  const result = handler(buildEvent('/blog/foo/bar/', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/blog/foo/bar/index.html')
})

test('/blog/rss.xml → pass through unchanged (file with extension)', () => {
  const result = handler(buildEvent('/blog/rss.xml', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/blog/rss.xml')
})

test('/blog/_astro/style.abc123.css → pass through unchanged (hashed asset)', () => {
  const result = handler(buildEvent('/blog/_astro/style.abc123.css', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/blog/_astro/style.abc123.css')
})

test('/blogfoo → NOT matched as /blog/, falls through to SPA rewrite', () => {
  // Boundary check: the /blog/ prefix must require the trailing slash so
  // unrelated top-level paths like /blogfoo or /blogfest are not hijacked.
  const result = handler(buildEvent('/blogfoo', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/index.html')
})

test('/blog/hello-world/ preserves querystring on rewrite', () => {
  const qs = { utm_source: { value: 'rss' } }
  const result = handler(buildEvent('/blog/hello-world/', 'iii.dev', qs))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/blog/hello-world/index.html')
  assert.equal(result.querystring, qs)
})

test('/blog/hello-world (no slash) preserves querystring on 301', () => {
  const result = handler(
    buildEvent('/blog/hello-world', 'iii.dev', { utm_source: { value: 'rss' } }),
  )
  assert.ok(isRedirect(result))
  assert.equal(locationOf(result), 'https://iii.dev/blog/hello-world/?utm_source=rss')
})

test('www.iii.dev/blog/hello-world/ → 301 apex (host check runs first)', () => {
  const result = handler(buildEvent('/blog/hello-world/', 'www.iii.dev'))
  assert.ok(isRedirect(result))
  assert.equal(locationOf(result), 'https://iii.dev/blog/hello-world/')
})

test('preview-host /blog/foo → 301 to same preview host (preserves origin)', () => {
  // Don't lock /blog/* redirects to apex — preview deploys must redirect to
  // the same hostname so reviewers stay on the preview environment.
  const result = handler(buildEvent('/blog/foo', 'preview.iii.dev'))
  assert.ok(isRedirect(result))
  assert.equal(locationOf(result), 'https://preview.iii.dev/blog/foo/')
})

test('missing host header → SPA fallback still applies for extensionless paths', () => {
  const event = buildEvent('/some/page', undefined)
  delete event.request.headers.host
  const result = handler(event)
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/index.html')
})
