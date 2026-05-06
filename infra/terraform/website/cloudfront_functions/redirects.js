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

// CloudFront Functions deliver request.querystring as
//   { key: { value: string, multiValue?: [{ value: string }, ...] } }
// where repeated params spill into multiValue. We re-encode and rejoin so the
// host-redirect path below preserves the original query (otherwise `?a=1&a=2`
// would silently drop on the 301).
function serializeQuerystring(qs) {
  if (!qs) return ''
  var parts = []
  for (var key in qs) {
    if (!Object.prototype.hasOwnProperty.call(qs, key)) continue
    var entry = qs[key]
    if (!entry) continue
    var encodedKey = encodeURIComponent(key)
    var primary = entry.value == null ? '' : entry.value
    parts.push(encodedKey + '=' + encodeURIComponent(primary))
    if (entry.multiValue && entry.multiValue.length) {
      for (var i = 0; i < entry.multiValue.length; i++) {
        var extra = entry.multiValue[i]
        var extraValue = extra && extra.value != null ? extra.value : ''
        parts.push(encodedKey + '=' + encodeURIComponent(extraValue))
      }
    }
  }
  return parts.length ? '?' + parts.join('&') : ''
}

// biome-ignore lint/correctness/noUnusedVariables: CloudFront Function entry point
// biome-ignore lint/complexity/useOptionalChain: cloudfront-js-2.0 does NOT support optional chaining
function handler(event) {
  var request = event.request
  var uri = request.uri
  var host = request.headers && request.headers.host ? request.headers.host.value : undefined

  if (host === 'www.iii.dev') {
    return redirect(`https://iii.dev${uri}${serializeQuerystring(request.querystring)}`)
  }

  if (uri.indexOf('/.well-known/') === 0) return request

  // Pretty URLs → matching *.html objects in S3 (Option A). Add a key when you
  // ship a new top-level page as `pagename.html`.
  var htmlPretty = {
    '/manifesto': '/manifesto.html',
  }
  var htmlTarget = htmlPretty[uri]
  if (htmlTarget !== undefined) {
    request.uri = htmlTarget
    return request
  }

  // /blog/* — Astro emits build.format: 'directory' with trailingSlash:
  // 'always', so canonical URLs are /blog/<slug>/. CloudFront's
  // default_root_object only applies to the apex, so we rewrite directory
  // URLs to .../index.html and 301 extensionless paths to the canonical
  // trailing-slash form. Must run before the SPA fallback so /blog/<slug>
  // doesn't get hijacked into /index.html.
  var redirectHost = host || 'iii.dev'
  if (uri === '/blog') {
    return redirect('https://' + redirectHost + '/blog/' + serializeQuerystring(request.querystring))
  }
  if (uri.indexOf('/blog/') === 0) {
    if (uri.charAt(uri.length - 1) === '/') {
      request.uri = uri + 'index.html'
      return request
    }
    var lastSlashB = uri.lastIndexOf('/')
    var lastSegmentB = uri.substring(lastSlashB + 1)
    if (lastSegmentB.indexOf('.') === -1) {
      return redirect('https://' + redirectHost + uri + '/' + serializeQuerystring(request.querystring))
    }
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
