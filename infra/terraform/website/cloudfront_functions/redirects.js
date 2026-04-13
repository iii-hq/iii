// CloudFront Function (cloudfront-js-2.0): viewer-request handler for iii.dev.
//
// Runs on every incoming request to the default (S3) behavior. Not attached to
// the /api/search* behavior — its JSON responses must pass through unchanged.
//
// Responsibilities, in order:
//   1. /docs or /docs/* → 301 docs.iii.dev, **preserving the /docs prefix**.
//      Why: the Mintlify project serves content under /docs/... Visiting
//      docs.iii.dev/<slug> (without /docs prefix) returns 404 in the current
//      setup. The k8s edge-proxy's docs.iii.dev server block has a matching
//      rewrite (^/docs/docs/(.*)$ → /docs/$1) that handles the double-prefix.
//      Until Mintlify is reconfigured to serve at the root of docs.iii.dev,
//      we must redirect to the same path the current proxy serves.
//   2. host www.iii.dev → 301 apex iii.dev (preserves path)
//   3. /.well-known/*  → pass through (S3 returns 404, no SPA rewrite)
//   4. SPA fallback: extensionless path that doesn't end in / → rewrite uri to /index.html
//   5. Everything else → pass through
//
// Why /docs is checked BEFORE www→apex: www.iii.dev/docs/foo should redirect
// directly to docs.iii.dev/docs/foo in ONE hop, not two.
//
// /llms.txt is intentionally NOT special-cased: the current live site returns
// 404 for iii.dev/llms.txt (Mintlify upstream doesn't have the content), so
// passing it through to S3 (which also 404s) preserves today's behavior.
//
// Tested in redirects.test.js via `node --test`.

function redirect(location) {
  return {
    statusCode: 301,
    statusDescription: 'Moved Permanently',
    headers: {
      'location': { value: location },
      'cache-control': { value: 'public, max-age=3600' },
    },
  };
}

// biome-ignore lint/correctness/noUnusedVariables: CloudFront Function entry point (called by CloudFront runtime, not imported)
function handler(event) {
  const request = event.request;
  const uri = request.uri;
  const host = request.headers?.host?.value;

  // 1. /docs exact → 301 docs.iii.dev/docs
  if (uri === '/docs') {
    return redirect('https://docs.iii.dev/docs');
  }

  // /docs/ exact → 301 docs.iii.dev/docs/
  if (uri === '/docs/') {
    return redirect('https://docs.iii.dev/docs/');
  }

  // /docs/<anything> → 301 docs.iii.dev/docs/<anything> (preserves the prefix)
  if (uri.indexOf('/docs/') === 0) {
    return redirect(`https://docs.iii.dev${uri}`);
  }

  // 2. www.iii.dev → 301 apex (preserves path; querystring reattached by CF)
  if (host === 'www.iii.dev') {
    return redirect(`https://iii.dev${uri}`);
  }

  // 3. /.well-known/* → pass through explicitly (S3 returns 404, no SPA fallback)
  if (uri.indexOf('/.well-known/') === 0) {
    return request;
  }

  // 4. SPA fallback: extensionless path, doesn't end with /, not the root.
  //    Rewrite URI to /index.html so S3 returns the SPA shell with 200.
  if (uri !== '/' && uri.charAt(uri.length - 1) !== '/') {
    const lastSlash = uri.lastIndexOf('/');
    const lastSegment = uri.substring(lastSlash + 1);
    if (lastSegment.indexOf('.') === -1) {
      request.uri = '/index.html';
      return request;
    }
  }

  // 5. Pass through unchanged.
  return request;
}
