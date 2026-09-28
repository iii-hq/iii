import { type ComponentType, type LazyExoticComponent, lazy, StrictMode, Suspense } from "react"
import { loadDeck } from "../generated/decks"

/**
 * The one client entry for every /roadmap/<slug>/deck/ page (mounted, browser
 * only, by website/src/app/(deck)/roadmap/[slug]/deck/). Decks stay
 * code-split: `loadDeck` (roadmap/generated/decks.ts, written before every
 * dev/build) is a switch of static `import()`s, so a page only downloads its
 * own deck's chunk — the same per-deck isolation the standalone per-deck Vite
 * builds used to provide.
 */
const cache = new Map<string, LazyExoticComponent<ComponentType>>()

function componentFor(slug: string): LazyExoticComponent<ComponentType> {
  let component = cache.get(slug)
  if (!component) {
    component = lazy(() => loadDeck(slug))
    cache.set(slug, component)
  }
  return component
}

export default function DeckHost({ slug }: { slug: string }) {
  const App = componentFor(slug)
  return (
    <StrictMode>
      <Suspense fallback={null}>
        <App />
      </Suspense>
    </StrictMode>
  )
}
