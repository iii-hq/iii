import type { Route } from '@lib/hooks/useHashRoute'
import type { NavItem } from '@lib/lib/deck-types'
import { cn } from '@lib/lib/utils'
import { useEffect, useState } from 'react'

/**
 * The deck's in-page section nav. Branding and theme switching live in the
 * shared site header (src/components/SiteNav.astro), rendered by the Astro
 * page above the deck island — this bar only carries the deck's own links:
 * section scroll-spy on the home route, "back to the overview" on sub-pages,
 * and the spec link.
 */
export function TopNav({
  route,
  nav,
  specHref = '#/spec',
}: {
  route: Route
  nav: NavItem[]
  /** set to null to hide the spec link (e.g. in the md-only viewer) */
  specHref?: string | null
}) {
  const [active, setActive] = useState<string | null>(null)

  // scroll-spy: highlight the section currently in view (home page only)
  useEffect(() => {
    if (route.kind !== 'home') return
    const sections = nav.map((l) => document.getElementById(l.id)).filter((el): el is HTMLElement => el !== null)
    if (sections.length === 0) return
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) setActive(entry.target.id)
        }
      },
      { rootMargin: '-20% 0px -70% 0px' },
    )
    for (const el of sections) observer.observe(el)
    return () => observer.disconnect()
  }, [route, nav])

  return (
    <header className="border-b border-rule bg-bg">
      <div className="flex items-center gap-x-5 px-4 py-2.5 @3xl:px-9">
        {route.kind === 'home' ? (
          <nav className="flex items-center gap-x-4 min-w-0 overflow-x-auto">
            {nav.map((link) => (
              <a
                key={link.id}
                href={`#${link.id}`}
                className={cn(
                  'font-mono text-[12px] lowercase py-1.5 transition-colors whitespace-nowrap',
                  active === link.id ? 'text-accent' : 'text-ink-faint hover:text-ink',
                )}
              >
                {link.label}
              </a>
            ))}
          </nav>
        ) : (
          <a href="#/" className="font-mono text-[12px] lowercase text-ink-faint hover:text-ink transition-colors">
            ← back to the overview
          </a>
        )}

        {specHref ? (
          <a
            href={specHref}
            className={cn(
              'ml-auto shrink-0 font-mono text-[12px] lowercase whitespace-nowrap transition-colors',
              route.kind === 'page' && route.slug === 'spec' ? 'text-accent' : 'text-ink-faint hover:text-ink',
            )}
          >
            spec
          </a>
        ) : null}
      </div>
    </header>
  )
}
