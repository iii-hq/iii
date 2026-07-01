// redirects.js is authored for the cloudfront-js-2.0 runtime and has no CJS/ESM
// wrapper, so we load it via `new Function(...)` rather than `require`. The
// runtime's `import cf from 'cloudfront'` is invalid inside a Function body, so
// we strip that line and inject a mock `cf` whose KeyValueStore is backed by an
// in-memory route map (production populates the real KVS from website/*.html).

const test = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')

const rawSource = fs.readFileSync(path.join(__dirname, 'redirects.js'), 'utf8')
const source = rawSource.replace(/^\s*import\s+cf\s+from\s+'cloudfront';?[^\n]*$/m, '')

// Bind a handler to a specific KVS route map. cf.kvs().get(key) resolves to the
// mapped value or rejects when the key is absent — mirroring the real
// CloudFront KeyValueStore data-plane semantics (a miss throws).
function makeHandler(routeMap) {
  const cf = {
    kvs() {
      return {
        get(key) {
          if (Object.prototype.hasOwnProperty.call(routeMap, key)) {
            return Promise.resolve(routeMap[key])
          }
          return Promise.reject(new Error('KeyNotFound: ' + key))
        },
      }
    },
  }
  return new Function('cf', source + '\nreturn handler;')(cf)
}

// Default map mirrors the two pretty pages that ship in website/ today.
const handler = makeHandler({
  '/manifesto': '/manifesto.html',
  '/privacy-policy': '/privacy-policy.html',
})

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

function isNotFound(result) {
  return result && typeof result === 'object' && result.statusCode === 404
}

// In production /docs and /docs/* are routed to the docs-nlb origin via a
// separate CloudFront behavior with no function_association, so this handler
// never runs for those paths. The unit assertions below describe the
// function's in-isolation behavior — they no longer rewrite to /index.html
// because the SPA fallback was replaced with a real 404 (was: soft-404
// homepage clone, see notFound() in redirects.js).
test('/docs → function returns 404 in isolation (production routes via docs behavior)', async () => {
  const result = await handler(buildEvent('/docs', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.ok(isNotFound(result))
})

test('/docs/quickstart → function returns 404 in isolation (production routes via docs behavior)', async () => {
  const result = await handler(buildEvent('/docs/quickstart', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.ok(isNotFound(result))
})

test('/docsfoo → 404 (not under /docs/, not a known pretty URL)', async () => {
  const result = await handler(buildEvent('/docsfoo', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.ok(isNotFound(result))
})

test('/llms.txt → pass through unchanged (static file)', async () => {
  const result = await handler(buildEvent('/llms.txt', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/llms.txt')
})

test('www.iii.dev/ → 301 https://iii.dev/', async () => {
  const result = await handler(buildEvent('/', 'www.iii.dev'))
  assert.ok(isRedirect(result))
  assert.equal(locationOf(result), 'https://iii.dev/')
})

test('www.iii.dev/some/page → 301 https://iii.dev/some/page', async () => {
  const result = await handler(buildEvent('/some/page', 'www.iii.dev'))
  assert.ok(isRedirect(result))
  assert.equal(locationOf(result), 'https://iii.dev/some/page')
})

test('www.iii.dev/docs/foo → 301 https://iii.dev/docs/foo', async () => {
  const result = await handler(buildEvent('/docs/foo', 'www.iii.dev'))
  assert.ok(isRedirect(result))
  assert.equal(locationOf(result), 'https://iii.dev/docs/foo')
})

test('www.iii.dev preserves querystring with multiValue and empty params', async () => {
  // Mirrors the CloudFront Functions querystring shape: repeated keys spill into
  // multiValue, value-less keys arrive as empty strings, and special chars must
  // be re-encoded.
  const result = await handler(
    buildEvent('/some/page', 'www.iii.dev', {
      a: { value: '1', multiValue: [{ value: '2' }] },
      empty: { value: '' },
      ref: { value: 'hello world' },
    }),
  )
  assert.ok(isRedirect(result))
  assert.equal(locationOf(result), 'https://iii.dev/some/page?a=1&a=2&empty=&ref=hello%20world')
})

test('www.iii.dev with no querystring → no trailing ?', async () => {
  const result = await handler(buildEvent('/some/page', 'www.iii.dev', {}))
  assert.ok(isRedirect(result))
  assert.equal(locationOf(result), 'https://iii.dev/some/page')
})

test('www.iii.dev percent-encodes reserved chars in keys and values', async () => {
  // Values containing &, =, #, + would otherwise corrupt the redirect target
  // (& splits params, # ends the URL into a fragment, + flips to space on parse,
  // = confuses some clients). Keys with spaces must also be encoded.
  const result = await handler(
    buildEvent('/p', 'www.iii.dev', {
      'weird key': { value: 'a&b=c+d#e' },
    }),
  )
  assert.ok(isRedirect(result))
  assert.equal(locationOf(result), 'https://iii.dev/p?weird%20key=a%26b%3Dc%2Bd%23e')
})

test('unknown extensionless path → 404 (was SPA fallback to /index.html)', async () => {
  const qs = { utm_source: { value: 'twitter' }, ref: { value: 'launch' } }
  const event = buildEvent('/some/route', 'iii.dev', qs)
  const result = await handler(event)
  assert.ok(!isRedirect(result))
  assert.ok(isNotFound(result))
})

test('/ (root) → pass through unchanged', async () => {
  const result = await handler(buildEvent('/', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/')
})

test('/some/client/route → 404 (no client-side routing on the static site)', async () => {
  const result = await handler(buildEvent('/some/client/route', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.ok(isNotFound(result))
})

test('/manifesto → rewrite uri to /manifesto.html (flat HTML, Option A)', async () => {
  const result = await handler(buildEvent('/manifesto', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/manifesto.html')
})

test('/manifesto.html → pass through unchanged', async () => {
  const result = await handler(buildEvent('/manifesto.html', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/manifesto.html')
})

test('/manifesto/ trailing slash → pass through (pretty-URL rewrite only matches exact extensionless path)', async () => {
  const result = await handler(buildEvent('/manifesto/', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/manifesto/')
})

test('www.iii.dev/manifesto → 301 https://iii.dev/manifesto (host-redirect runs before pretty-URL rewrite)', async () => {
  const result = await handler(buildEvent('/manifesto', 'www.iii.dev'))
  assert.ok(isRedirect(result))
  assert.equal(locationOf(result), 'https://iii.dev/manifesto')
})

test('/privacy-policy → rewrite uri to /privacy-policy.html (flat HTML, Option A)', async () => {
  const result = await handler(buildEvent('/privacy-policy', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/privacy-policy.html')
})

test('/privacy-policy.html → pass through unchanged', async () => {
  const result = await handler(buildEvent('/privacy-policy.html', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/privacy-policy.html')
})

test('www.iii.dev/privacy-policy → 301 https://iii.dev/privacy-policy (host-redirect runs before pretty-URL rewrite)', async () => {
  const result = await handler(buildEvent('/privacy-policy', 'www.iii.dev'))
  assert.ok(isRedirect(result))
  assert.equal(locationOf(result), 'https://iii.dev/privacy-policy')
})

test('/AGENTS.md → pass through unchanged', async () => {
  const result = await handler(buildEvent('/AGENTS.md', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/AGENTS.md')
})

test('/foo/ trailing slash → pass through unchanged (no SPA rewrite)', async () => {
  const result = await handler(buildEvent('/foo/', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/foo/')
})

test('/missing.jpg → pass through unchanged (S3 returns 404)', async () => {
  const result = await handler(buildEvent('/missing.jpg', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/missing.jpg')
})

test('/ai → 404 (no /ai route on the static site; was soft-404 homepage clone)', async () => {
  const result = await handler(buildEvent('/ai', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.ok(isNotFound(result))
})

test('/assets/main.abc123.js → pass through unchanged', async () => {
  const result = await handler(buildEvent('/assets/main.abc123.js', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/assets/main.abc123.js')
})

test('/favicon.svg → pass through unchanged', async () => {
  const result = await handler(buildEvent('/favicon.svg', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/favicon.svg')
})

test('/.well-known/vercel/project.json → pass through (no SPA rewrite)', async () => {
  const result = await handler(buildEvent('/.well-known/vercel/project.json', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/.well-known/vercel/project.json')
})

test('/.well-known/foo (no extension) → pass through, NOT SPA rewritten', async () => {
  const result = await handler(buildEvent('/.well-known/foo', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/.well-known/foo', '.well-known is an explicit exemption from SPA fallback')
})

// ---------------------------------------------------------------------------
// Directory-style sites — /blog/* (Astro, build.format 'directory' with
// trailingSlash 'always') and /tech-specs/* (the presentations build: gallery
// + one directory per spec). Each page is a key like <prefix>/<slug>/index.html
// in S3. CloudFront's default_root_object only applies to the apex, so
// /<prefix>/<slug>/ must be rewritten to .../index.html and extensionless
// paths must 301 to the canonical trailing-slash form. Same behavior for both
// prefixes, so the cases are parameterized.
// ---------------------------------------------------------------------------

for (const prefix of ['/blog', '/tech-specs']) {
  test(`${prefix} → 301 https://iii.dev${prefix}/ (canonical trailing slash)`, async () => {
    const result = await handler(buildEvent(prefix, 'iii.dev'))
    assert.ok(isRedirect(result))
    assert.equal(locationOf(result), `https://iii.dev${prefix}/`)
  })

  test(`${prefix}/ → rewrite to ${prefix}/index.html`, async () => {
    const result = await handler(buildEvent(`${prefix}/`, 'iii.dev'))
    assert.ok(!isRedirect(result))
    assert.equal(result.uri, `${prefix}/index.html`)
  })

  test(`${prefix}/hello-world/ → rewrite to ${prefix}/hello-world/index.html`, async () => {
    const result = await handler(buildEvent(`${prefix}/hello-world/`, 'iii.dev'))
    assert.ok(!isRedirect(result))
    assert.equal(result.uri, `${prefix}/hello-world/index.html`)
  })

  test(`${prefix}/hello-world (no trailing slash) → 301 to ${prefix}/hello-world/`, async () => {
    const result = await handler(buildEvent(`${prefix}/hello-world`, 'iii.dev'))
    assert.ok(isRedirect(result))
    assert.equal(locationOf(result), `https://iii.dev${prefix}/hello-world/`)
  })

  test(`${prefix}/foo/bar/ → rewrite to ${prefix}/foo/bar/index.html (nested)`, async () => {
    const result = await handler(buildEvent(`${prefix}/foo/bar/`, 'iii.dev'))
    assert.ok(!isRedirect(result))
    assert.equal(result.uri, `${prefix}/foo/bar/index.html`)
  })

  test(`${prefix}/rss.xml → pass through unchanged (file with extension)`, async () => {
    const result = await handler(buildEvent(`${prefix}/rss.xml`, 'iii.dev'))
    assert.ok(!isRedirect(result))
    assert.equal(result.uri, `${prefix}/rss.xml`)
  })

  test(`${prefix}/_astro/style.abc123.css → pass through unchanged (hashed asset)`, async () => {
    const result = await handler(buildEvent(`${prefix}/_astro/style.abc123.css`, 'iii.dev'))
    assert.ok(!isRedirect(result))
    assert.equal(result.uri, `${prefix}/_astro/style.abc123.css`)
  })

  test(`${prefix}foo → NOT matched as ${prefix}/, falls through to 404`, async () => {
    // Boundary check: the prefix must require the trailing slash so unrelated
    // top-level paths like /blogfoo or /tech-specsfoo are not hijacked.
    const result = await handler(buildEvent(`${prefix}foo`, 'iii.dev'))
    assert.ok(!isRedirect(result))
    assert.ok(isNotFound(result))
  })
}

test('/tech-specs/index.json → pass through unchanged (the landing-page feed)', async () => {
  const result = await handler(buildEvent('/tech-specs/index.json', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/tech-specs/index.json')
})

test('/tech-specs/2026-06-codegen/harness.md → pass through unchanged (raw spec md)', async () => {
  const result = await handler(buildEvent('/tech-specs/2026-06-codegen/harness.md', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/tech-specs/2026-06-codegen/harness.md')
})

test('/blog/hello-world/ preserves querystring on rewrite', async () => {
  const qs = { utm_source: { value: 'rss' } }
  const result = await handler(buildEvent('/blog/hello-world/', 'iii.dev', qs))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/blog/hello-world/index.html')
  assert.equal(result.querystring, qs)
})

test('/blog/hello-world (no slash) preserves querystring on 301', async () => {
  const result = await handler(buildEvent('/blog/hello-world', 'iii.dev', { utm_source: { value: 'rss' } }))
  assert.ok(isRedirect(result))
  assert.equal(locationOf(result), 'https://iii.dev/blog/hello-world/?utm_source=rss')
})

test('www.iii.dev/blog/hello-world/ → 301 apex (host check runs first)', async () => {
  const result = await handler(buildEvent('/blog/hello-world/', 'www.iii.dev'))
  assert.ok(isRedirect(result))
  assert.equal(locationOf(result), 'https://iii.dev/blog/hello-world/')
})

test('preview-host /blog/foo → 301 to same preview host (preserves origin)', async () => {
  // Don't lock /blog/* redirects to apex — preview deploys must redirect to
  // the same hostname so reviewers stay on the preview environment.
  const result = await handler(buildEvent('/blog/foo', 'preview.iii.dev'))
  assert.ok(isRedirect(result))
  assert.equal(locationOf(result), 'https://preview.iii.dev/blog/foo/')
})

test('missing host header → still 404 for unknown extensionless paths', async () => {
  const event = buildEvent('/some/page', undefined)
  delete event.request.headers.host
  const result = await handler(event)
  assert.ok(!isRedirect(result))
  assert.ok(isNotFound(result))
})

// ---------------------------------------------------------------------------
// KVS route map — pretty-URL resolution now reads a CloudFront KeyValueStore
// instead of a hardcoded object, so adding a page is a content-only change
// (deploy-website.yml syncs the store from website/*.html; no terraform apply).
// These cases exercise the lookup against bespoke maps via makeHandler().
// ---------------------------------------------------------------------------

test('KVS hit: a page present only in the store resolves with no code change', async () => {
  const h = makeHandler({ '/pricing': '/pricing.html' })
  const result = await h(buildEvent('/pricing', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/pricing.html')
})

test('KVS miss: extensionless path absent from the store → real 404', async () => {
  const h = makeHandler({ '/manifesto': '/manifesto.html' })
  const result = await h(buildEvent('/privacy-policy', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.ok(isNotFound(result))
})

test('KVS value is honored verbatim (key and value need not share a stem)', async () => {
  const h = makeHandler({ '/legal': '/privacy-policy.html' })
  const result = await h(buildEvent('/legal', 'iii.dev'))
  assert.ok(!isRedirect(result))
  assert.equal(result.uri, '/privacy-policy.html')
})

test('empty store: every extensionless path 404s, static files still pass through', async () => {
  const h = makeHandler({})
  const notFoundResult = await h(buildEvent('/manifesto', 'iii.dev'))
  assert.ok(isNotFound(notFoundResult))
  const assetResult = await h(buildEvent('/favicon.svg', 'iii.dev'))
  assert.ok(!isRedirect(assetResult))
  assert.equal(assetResult.uri, '/favicon.svg')
})

test('www host redirects before any KVS lookup (host check wins)', async () => {
  const h = makeHandler({ '/pricing': '/pricing.html' })
  const result = await h(buildEvent('/pricing', 'www.iii.dev'))
  assert.ok(isRedirect(result))
  assert.equal(locationOf(result), 'https://iii.dev/pricing')
})
