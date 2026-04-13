// Unit tests for the CloudFront Function at redirects.js.
//
// Run with:  node --test infra/terraform/website/cloudfront_functions/redirects.test.js
//
// The CloudFront Functions runtime (cloudfront-js-2.0) is close to ES2020 with no
// Node built-ins. redirects.js defines `handler(event)` at module scope without
// `module.exports`, so we load it via `fs.readFileSync` + `new Function(...)`
// rather than `require(...)`. That lets us test the pure function in plain Node
// without shipping a CommonJS wrapper to CloudFront.

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const source = fs.readFileSync(
  path.join(__dirname, 'redirects.js'),
  'utf8',
);

// Load `handler` into the current scope by evaluating the source inside a
// fresh Function sandbox. Returns the `handler` symbol.
const handler = new Function(
  source + '\nreturn handler;',
)();

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
  };
}

function isRedirect(result) {
  return (
    result &&
    typeof result === 'object' &&
    result.statusCode === 301 &&
    result.headers &&
    result.headers.location &&
    typeof result.headers.location.value === 'string'
  );
}

function locationOf(result) {
  return result.headers.location.value;
}

// ── /docs redirect cases (preserves the /docs prefix) ──────────────────────
// Why preserved: Mintlify docs project only serves content under /docs/..
// docs.iii.dev/quickstart returns 404, docs.iii.dev/docs/quickstart works.
// See redirects.js header comment for the full rationale.

test('/docs exact → 301 https://docs.iii.dev/docs', () => {
  const result = handler(buildEvent('/docs', 'iii.dev'));
  assert.ok(isRedirect(result));
  assert.equal(locationOf(result), 'https://docs.iii.dev/docs');
});

test('/docs/ trailing slash → 301 https://docs.iii.dev/docs/', () => {
  const result = handler(buildEvent('/docs/', 'iii.dev'));
  assert.ok(isRedirect(result));
  assert.equal(locationOf(result), 'https://docs.iii.dev/docs/');
});

test('/docs/quickstart → 301 https://docs.iii.dev/docs/quickstart', () => {
  const result = handler(buildEvent('/docs/quickstart', 'iii.dev'));
  assert.ok(isRedirect(result));
  assert.equal(locationOf(result), 'https://docs.iii.dev/docs/quickstart');
});

test('/docs/guide/deep/nested → preserves deep path on redirect', () => {
  const result = handler(buildEvent('/docs/guide/deep/nested', 'iii.dev'));
  assert.ok(isRedirect(result));
  assert.equal(locationOf(result), 'https://docs.iii.dev/docs/guide/deep/nested');
});

test('/docsfoo → NOT redirected (not under /docs/)', () => {
  const result = handler(buildEvent('/docsfoo', 'iii.dev'));
  assert.ok(!isRedirect(result), 'should not be a redirect');
  // SPA fallback applies: extensionless, no trailing slash
  assert.equal(result.uri, '/index.html');
});

// ── /llms.txt: no special handling ─────────────────────────────────────────
// Rationale: the current live site returns 404 for iii.dev/llms.txt, so
// passing it through to S3 (also 404) preserves today's behavior. No point
// redirecting to a URL that also 404s.

test('/llms.txt → pass through unchanged (matches current 404 behavior)', () => {
  const result = handler(buildEvent('/llms.txt', 'iii.dev'));
  assert.ok(!isRedirect(result));
  assert.equal(result.uri, '/llms.txt');
});

// ── www → apex redirect ────────────────────────────────────────────────────

test('www.iii.dev/ → 301 https://iii.dev/', () => {
  const result = handler(buildEvent('/', 'www.iii.dev'));
  assert.ok(isRedirect(result));
  assert.equal(locationOf(result), 'https://iii.dev/');
});

test('www.iii.dev/some/page → 301 https://iii.dev/some/page', () => {
  const result = handler(buildEvent('/some/page', 'www.iii.dev'));
  assert.ok(isRedirect(result));
  assert.equal(locationOf(result), 'https://iii.dev/some/page');
});

test('www.iii.dev/docs/foo → 301 https://docs.iii.dev/docs/foo (ONE hop, not two)', () => {
  const result = handler(buildEvent('/docs/foo', 'www.iii.dev'));
  assert.ok(isRedirect(result));
  assert.equal(
    locationOf(result),
    'https://docs.iii.dev/docs/foo',
    'docs redirect must win over www→apex redirect to avoid a 2-hop chain',
  );
});

// ── SPA fallback ───────────────────────────────────────────────────────────

test('/ (root) → pass through unchanged', () => {
  const result = handler(buildEvent('/', 'iii.dev'));
  assert.ok(!isRedirect(result));
  assert.equal(result.uri, '/');
});

test('/some/client/route → rewrite uri to /index.html', () => {
  const result = handler(buildEvent('/some/client/route', 'iii.dev'));
  assert.ok(!isRedirect(result));
  assert.equal(result.uri, '/index.html');
});

test('/manifesto → rewrite uri to /index.html', () => {
  const result = handler(buildEvent('/manifesto', 'iii.dev'));
  assert.ok(!isRedirect(result));
  assert.equal(result.uri, '/index.html');
});

test('/foo/ trailing slash → pass through unchanged (no SPA rewrite)', () => {
  const result = handler(buildEvent('/foo/', 'iii.dev'));
  assert.ok(!isRedirect(result));
  assert.equal(result.uri, '/foo/');
});

// ── File extension pass-through (NOT SPA fallback) ─────────────────────────

test('/missing.jpg → pass through unchanged (S3 returns 404)', () => {
  const result = handler(buildEvent('/missing.jpg', 'iii.dev'));
  assert.ok(!isRedirect(result));
  assert.equal(result.uri, '/missing.jpg');
});

test('/ai/index.html → pass through unchanged', () => {
  const result = handler(buildEvent('/ai/index.html', 'iii.dev'));
  assert.ok(!isRedirect(result));
  assert.equal(result.uri, '/ai/index.html');
});

test('/assets/main.abc123.js → pass through unchanged', () => {
  const result = handler(buildEvent('/assets/main.abc123.js', 'iii.dev'));
  assert.ok(!isRedirect(result));
  assert.equal(result.uri, '/assets/main.abc123.js');
});

test('/favicon.svg → pass through unchanged', () => {
  const result = handler(buildEvent('/favicon.svg', 'iii.dev'));
  assert.ok(!isRedirect(result));
  assert.equal(result.uri, '/favicon.svg');
});

// ── /.well-known exception ─────────────────────────────────────────────────

test('/.well-known/vercel/project.json → pass through (no SPA rewrite)', () => {
  const result = handler(buildEvent('/.well-known/vercel/project.json', 'iii.dev'));
  assert.ok(!isRedirect(result));
  assert.equal(result.uri, '/.well-known/vercel/project.json');
});

test('/.well-known/foo (no extension) → pass through, NOT SPA rewritten', () => {
  const result = handler(buildEvent('/.well-known/foo', 'iii.dev'));
  assert.ok(!isRedirect(result));
  assert.equal(
    result.uri,
    '/.well-known/foo',
    '.well-known is an explicit exemption from SPA fallback',
  );
});

// ── Missing host header (defensive) ────────────────────────────────────────

test('missing host header → still handles other rules correctly', () => {
  const event = buildEvent('/docs/foo', undefined);
  delete event.request.headers.host;
  const result = handler(event);
  assert.ok(isRedirect(result));
  assert.equal(locationOf(result), 'https://docs.iii.dev/docs/foo');
});
