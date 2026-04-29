// Viewer-request handler for the default (S3) behavior. Tested in redirects.test.js.

function redirect(location) {
  return {
    statusCode: 301,
    statusDescription: 'Moved Permanently',
    headers: {
      location: { value: location },
      'cache-control': { value: 'public, max-age=3600' },
    },
  }
}

// cleanUrls allowlist: extensionless paths that map to a real .html file in S3.
// Mirrors `cleanUrls: true` from website/vercel.json, which was lost in the
// migration from Vercel to S3+CloudFront. Add new static-page slugs here.
var CLEAN_URL_PAGES = {
  '/manifesto': '/manifesto.html',
}

// biome-ignore lint/correctness/noUnusedVariables: CloudFront Function entry point
// biome-ignore lint/complexity/useOptionalChain: cloudfront-js-2.0 does NOT support optional chaining
function handler(event) {
  var request = event.request
  var uri = request.uri
  var host = request.headers && request.headers.host ? request.headers.host.value : undefined

  if (host === 'www.iii.dev') return redirect(`https://iii.dev${uri}`)

  if (uri.indexOf('/.well-known/') === 0) return request

  if (CLEAN_URL_PAGES[uri]) {
    request.uri = CLEAN_URL_PAGES[uri]
    return request
  }

  // SPA fallback: extensionless path not ending in /
  if (uri !== '/' && uri.charAt(uri.length - 1) !== '/') {
    const lastSlash = uri.lastIndexOf('/')
    const lastSegment = uri.substring(lastSlash + 1)
    if (lastSegment.indexOf('.') === -1) {
      request.uri = '/index.html'
      return request
    }
  }

  return request
}
