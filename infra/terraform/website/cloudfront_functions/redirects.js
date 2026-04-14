// Viewer-request handler for the default (S3) behavior. Tested in redirects.test.js.
// /docs redirects preserve the /docs prefix because Mintlify serves content under /docs/...

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

// biome-ignore lint/correctness/noUnusedVariables: CloudFront Function entry point
// biome-ignore lint/complexity/useOptionalChain: cloudfront-js-2.0 does NOT support optional chaining
function handler(event) {
  var request = event.request
  var uri = request.uri
  var host = request.headers && request.headers.host ? request.headers.host.value : undefined

  if (uri === '/docs') return redirect('https://docs.iii.dev/docs')
  if (uri === '/docs/') return redirect('https://docs.iii.dev/docs/')
  if (uri.indexOf('/docs/') === 0) return redirect(`https://docs.iii.dev${uri}`)

  if (host === 'www.iii.dev') return redirect(`https://iii.dev${uri}`)

  if (uri.indexOf('/.well-known/') === 0) return request

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
