// CloudFront Function (cloudfront-js-2.0): viewer-request handler for iii.dev.
//
// Runs on every incoming request to the default (S3) behavior. Not attached to
// the /api/search* behavior — its JSON responses must pass through unchanged.
//
// Responsibilities, in order:
//   1. /docs or /docs/* → 301 docs.iii.dev (strips /docs prefix)
//   2. /llms.txt       → 301 docs.iii.dev/llms.txt
//   3. host www.iii.dev → 301 apex iii.dev (preserves path)
//   4. /.well-known/*  → pass through (S3 returns 404, no SPA rewrite)
//   5. SPA fallback: extensionless path that doesn't end in / → rewrite uri to /index.html
//   6. Everything else → pass through
//
// Why /docs is checked BEFORE www→apex: www.iii.dev/docs/foo should redirect
// directly to docs.iii.dev/foo in ONE hop, not two (www → apex → docs).
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

function handler(event) {
  var request = event.request;
  var uri = request.uri;
  var host = request.headers && request.headers.host && request.headers.host.value;

  // 1. /docs exact or /docs/ exact → 301 docs.iii.dev/
  if (uri === '/docs' || uri === '/docs/') {
    return redirect('https://docs.iii.dev/');
  }

  // /docs/<anything> → 301 docs.iii.dev/<anything>
  if (uri.indexOf('/docs/') === 0) {
    return redirect('https://docs.iii.dev' + uri.substring('/docs'.length));
  }

  // 2. /llms.txt → 301 docs.iii.dev/llms.txt
  if (uri === '/llms.txt') {
    return redirect('https://docs.iii.dev/llms.txt');
  }

  // 3. www.iii.dev → 301 apex (preserves path including querystring handled by CF)
  if (host === 'www.iii.dev') {
    return redirect('https://iii.dev' + uri);
  }

  // 4. /.well-known/* → pass through explicitly (S3 returns 404, no SPA fallback)
  if (uri.indexOf('/.well-known/') === 0) {
    return request;
  }

  // 5. SPA fallback: extensionless path, doesn't end with /, not the root.
  //    Rewrite URI to /index.html so S3 returns the SPA shell with 200.
  if (uri !== '/' && uri.charAt(uri.length - 1) !== '/') {
    var lastSlash = uri.lastIndexOf('/');
    var lastSegment = uri.substring(lastSlash + 1);
    if (lastSegment.indexOf('.') === -1) {
      request.uri = '/index.html';
      return request;
    }
  }

  // 6. Pass through unchanged.
  return request;
}
