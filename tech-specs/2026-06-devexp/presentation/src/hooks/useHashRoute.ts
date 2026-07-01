import { useEffect, useState } from 'react'

export type PageId = 'playground' | 'compose' | 'migration'

export type Route = { kind: 'home' } | { kind: 'page'; id: PageId }

const PAGES: PageId[] = ['playground', 'compose', 'migration']

function parse(hash: string): Route {
  const m = hash.match(/^#\/(playground|compose|migration)$/)
  if (m && PAGES.includes(m[1] as PageId)) {
    return { kind: 'page', id: m[1] as PageId }
  }
  return { kind: 'home' }
}

/**
 * hash routing with two namespaces: `#/...` paths are routes (deep-dive
 * pages); bare `#section-id` hashes stay native anchor scrolls on the home
 * page.
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
