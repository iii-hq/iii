import { type ComponentType, type LazyExoticComponent, lazy, StrictMode, Suspense } from 'react'

/**
 * The one client entry for every /roadmap/<slug>/ page (mounted as an Astro
 * island by src/pages/roadmap/[slug]/index.astro). Decks stay code-split:
 * the glob is lazy, so a page only downloads its own deck's chunk — the same
 * per-deck isolation the standalone per-deck Vite builds used to provide.
 * Specs without a deck get the generic markdown viewer.
 */
const decks = import.meta.glob('../*/src/App.tsx') as Record<string, () => Promise<{ default: ComponentType }>>

const cache = new Map<string, LazyExoticComponent<ComponentType>>()

function componentFor(slug: string, hasDeck: boolean): LazyExoticComponent<ComponentType> {
  const key = hasDeck ? `../${slug}/src/App.tsx` : 'viewer'
  let component = cache.get(key)
  if (!component) {
    const load = hasDeck ? decks[key] : undefined
    component = load
      ? lazy(load)
      : lazy(() => import('../_viewer/src/ViewerApp').then((m) => ({ default: m.ViewerApp })))
    cache.set(key, component)
  }
  return component
}

export default function DeckHost({ slug, hasDeck }: { slug: string; hasDeck: boolean }) {
  const App = componentFor(slug, hasDeck)
  return (
    <StrictMode>
      <Suspense fallback={null}>
        <App />
      </Suspense>
    </StrictMode>
  )
}
