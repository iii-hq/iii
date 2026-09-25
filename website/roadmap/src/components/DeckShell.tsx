import { Sheet } from "@lib/components/schematic/Sheet"
import { TopNav } from "@lib/components/TopNav"
import type { Route } from "@lib/hooks/useHashRoute"
import type { NavItem } from "@lib/lib/deck-types"
import type { ComponentType, ReactNode } from "react"

/**
 * the frame every deck App renders: container-query root, drafting sheet,
 * top nav, then whichever of home / a `#/<slug>` page / not-found the route
 * selects. decks own their section order (`home`) and page registry (`pages`).
 */
export function DeckShell({
  route,
  nav,
  pages,
  home,
}: {
  route: Route
  nav: NavItem[]
  /** deep-dive pages, keyed by the `#/<slug>` route slug */
  pages: Record<string, ComponentType>
  /** the ordered home-page sections */
  home: ReactNode
}) {
  const Page = route.kind === "page" ? pages[route.slug] : undefined
  return (
    <div className="@container min-h-screen">
      <Sheet>
        <TopNav route={route} nav={nav} />
        {route.kind === "home" ? home : Page ? <Page /> : <NotFound />}
      </Sheet>
    </div>
  )
}

function NotFound() {
  return (
    <main className="px-4 py-24 @3xl:px-9">
      <p className="font-mono text-[14px] lowercase text-ink-faint">
        nothing here.{" "}
        <a href="#/" className="text-ink hover:text-accent transition-colors">
          ← back to the overview
        </a>
      </p>
    </main>
  )
}
