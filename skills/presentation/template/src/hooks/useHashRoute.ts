import { useEffect, useState } from 'react'

export type Route =
  | { kind: 'home' }
  | { kind: 'page'; slug: string; rest: string[] }

function parse(hash: string): Route {
  // `#/<slug>` (and `#/<slug>/<sub>/...`) are page routes; the slug is the
  // FIRST segment, matched against the PAGES registry in App.tsx, and any
  // further segments are exposed as `rest` (e.g. the spec viewer's file).
  const m = hash.match(/^#\/(.+)$/)
  if (m) {
    const segments = m[1].split('/').filter(Boolean)
    const [slug, ...rest] = segments
    if (slug) return { kind: 'page', slug, rest }
  }
  return { kind: 'home' }
}

/**
 * hash routing with two namespaces: `#/...` paths are routes (deep-dive
 * pages); bare `#section-id` hashes stay native anchor scrolls on the home
 * page. unknown page slugs fall back to home (App renders a not-found note).
 */
export function useHashRoute(): Route {
  const [route, setRoute] = useState<Route>(() => parse(window.location.hash))

  useEffect(() => {
    const onChange = () => setRoute(parse(window.location.hash))
    window.addEventListener('hashchange', onChange)
    return () => window.removeEventListener('hashchange', onChange)
  }, [])

  useEffect(() => {
    // deep-dive pages and the explicit "#/" home link start at the top;
    // bare "#section" hashes keep native anchor behaviour.
    if (route.kind === 'page' || window.location.hash === '#/') {
      window.scrollTo({ top: 0, behavior: 'instant' })
    }
  }, [route])

  return route
}
