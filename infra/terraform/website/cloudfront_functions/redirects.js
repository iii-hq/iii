// Viewer-request handler for the default (S3) behavior. Tested in redirects.test.js.

import cf from 'cloudfront'

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

// Return a real 404 for unknown extensionless paths instead of the previous
// SPA fallback to /index.html. The old behavior served homepage HTML with
// status 200 for any unknown path (e.g. /workers/iii-queue from broken docs
// links), which Google indexed as soft-duplicate-of-homepage. Returning 404
// here lets Search Console's "Not found (404)" bucket reflect reality and
// stops generating "Duplicate without user-selected canonical" entries.
function notFound() {
  return {
    statusCode: 404,
    statusDescription: 'Not Found',
    headers: {
      'content-type': { value: 'text/html; charset=utf-8' },
      'cache-control': { value: 'public, max-age=300' },
    },
    body: '<!doctype html><meta charset="utf-8"><title>404 — iii</title><meta name="robots" content="noindex"><style>body{font-family:system-ui,sans-serif;max-width:40rem;margin:4rem auto;padding:0 1rem;color:#1a1a1a}a{color:#1a5fbf}</style><h1>404 — page not found</h1><p>That URL doesn\'t exist on iii.dev.</p><p><a href="/">home</a> · <a href="/docs">docs</a> · <a href="/blog/">blog</a> · <a href="/manifesto">manifesto</a></p>',
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

// Pretty-URL route map (/<page> → /<page>.html) lives in a CloudFront
// KeyValueStore, populated by deploy-website.yml from the set of website/*.html
// files. Adding a page needs no code change and no `terraform apply` — see
// infra/terraform/website/README.md. The function is associated with exactly
// one store, so cf.kvs() needs no store id.
var routes = cf.kvs()

// Tech-spec dirs renamed to day-precision slugs (2026-07): the old month-only
// URLs were already published (sitemap, social links), so 301 them — path
// suffix and query preserved. Extend this map on any future spec rename.
var TECH_SPEC_RENAMES = {
  '2026-06-agentic': '2026-06-08-agentic',
  '2026-06-rbac-proxy-worker': '2026-06-22-rbac-proxy-worker',
  '2026-06-codegen': '2026-06-29-codegen',
}

// biome-ignore lint/correctness/noUnusedVariables: CloudFront Function entry point
// biome-ignore lint/complexity/useOptionalChain: cloudfront-js-2.0 does NOT support optional chaining
async function handler(event) {
  var request = event.request
  var uri = request.uri
  var host = request.headers && request.headers.host ? request.headers.host.value : undefined

  if (host === 'www.iii.dev') {
    return redirect(`https://iii.dev${uri}${serializeQuerystring(request.querystring)}`)
  }

  if (uri.indexOf('/.well-known/') === 0) return request

  // Directory-style sites: /blog/* (Astro, build.format 'directory') and
  // /tech-specs/* (the presentations build — a gallery at the prefix root plus
  // one directory per spec). Canonical URLs carry a trailing slash.
  // CloudFront's default_root_object only applies to the apex, so we rewrite
  // directory URLs to .../index.html and 301 extensionless paths to the
  // canonical trailing-slash form (relative-base decks resolve ./assets/*
  // correctly only under the slash form). Must run before the KVS fallback so
  // /<prefix>/<slug> doesn't 404.
  var redirectHost = host || 'iii.dev'

  // Renamed tech-spec slugs 301 to their new home before the generic
  // directory handling (which would otherwise rewrite them to a now-missing
  // <old-slug>/index.html).
  if (uri.indexOf('/tech-specs/') === 0) {
    var specRest = uri.substring('/tech-specs/'.length)
    var specSlash = specRest.indexOf('/')
    var specSlug = specSlash === -1 ? specRest : specRest.substring(0, specSlash)
    if (Object.prototype.hasOwnProperty.call(TECH_SPEC_RENAMES, specSlug)) {
      var specSuffix = specSlash === -1 ? '/' : specRest.substring(specSlash)
      return redirect(
        'https://' +
          redirectHost +
          '/tech-specs/' +
          TECH_SPEC_RENAMES[specSlug] +
          specSuffix +
          serializeQuerystring(request.querystring),
      )
    }
  }

  var DIR_SITES = ['/blog', '/tech-specs']
  for (var d = 0; d < DIR_SITES.length; d++) {
    var prefix = DIR_SITES[d]
    if (uri === prefix) {
      return redirect('https://' + redirectHost + prefix + '/' + serializeQuerystring(request.querystring))
    }
    if (uri.indexOf(prefix + '/') === 0) {
      if (uri.charAt(uri.length - 1) === '/') {
        request.uri = uri + 'index.html'
        return request
      }
      var lastSlashD = uri.lastIndexOf('/')
      var lastSegmentD = uri.substring(lastSlashD + 1)
      if (lastSegmentD.indexOf('.') === -1) {
        return redirect('https://' + redirectHost + uri + '/' + serializeQuerystring(request.querystring))
      }
      return request
    }
  }

  // Extensionless top-level paths resolve to a pretty page via the KVS route
  // map (e.g. /privacy-policy → /privacy-policy.html), or return a real 404.
  // Previously the map was hardcoded here, so a new page needed a `terraform
  // apply` to republish the function; it now lives in a CloudFront
  // KeyValueStore that deploy-website.yml syncs from website/*.html, making a
  // new page a content-only change. See notFound() for why unknown paths 404
  // instead of falling back to /index.html.
  if (uri !== '/' && uri.charAt(uri.length - 1) !== '/') {
    const lastSlash = uri.lastIndexOf('/')
    const lastSegment = uri.substring(lastSlash + 1)
    if (lastSegment.indexOf('.') === -1) {
      try {
        request.uri = await routes.get(uri)
        return request
      } catch (err) {
        return notFound()
      }
    }
  }

  return request
}
