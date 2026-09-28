import { useEffect, useMemo, useSyncExternalStore } from "react"

export type Route = { kind: "home" } | { kind: "page"; slug: string; rest: string[] }

function parse(hash: string): Route {
  // `#/<slug>` (and `#/<slug>/<sub>/...`) are page routes; the slug is the
  // FIRST segment, matched against the PAGES registry in App.tsx, and any
  // further segments are exposed as `rest` (e.g. the spec viewer's file).
  const m = hash.match(/^#\/(.+)$/)
  if (m) {
    const segments = m[1].split("/").filter(Boolean)
    const [slug, ...rest] = segments
    if (slug) return { kind: "page", slug, rest }
  }
  return { kind: "home" }
}

function subscribe(onChange: () => void): () => void {
  window.addEventListener("hashchange", onChange)
  return () => window.removeEventListener("hashchange", onChange)
}

function getSnapshot(): string {
  return window.location.hash
}

// no hash on the server: every deck starts on its home route
function getServerSnapshot(): string {
  return ""
}

/**
 * hash routing with two namespaces: `#/...` paths are routes (deep-dive
 * pages); bare `#section-id` hashes stay native anchor scrolls on the home
 * page. unknown page slugs fall back to home (App renders a not-found note).
 */
export function useHashRoute(): Route {
  const hash = useSyncExternalStore(subscribe, getSnapshot, getServerSnapshot)
  const route = useMemo(() => parse(hash), [hash])

  useEffect(() => {
    // deep-dive pages and the explicit "#/" home link start at the top;
    // bare "#section" hashes keep native anchor behaviour.
    if (route.kind === "page" || hash === "#/") {
      window.scrollTo({ top: 0, behavior: "instant" })
    }
  }, [route, hash])

  return route
}
