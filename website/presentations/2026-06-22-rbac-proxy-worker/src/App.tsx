import { Footer } from '@lib/components/Footer'
import { Sheet } from '@lib/components/schematic/Sheet'
import { TopNav } from '@lib/components/TopNav'
import { useHashRoute } from '@lib/hooks/useHashRoute'
import { SpecPage } from '@lib/pages/SpecPage'
import type { ComponentType } from 'react'
import { DECK_META, FOOTER, NAV } from './content/deck'
import { EngineOverridesPage } from './pages/EngineOverridesPage'
import { RbacContractPage } from './pages/RbacContractPage'
import { AccessSection } from './sections/AccessSection'
import { CoexistenceSection } from './sections/CoexistenceSection'
import { FailClosedSection } from './sections/FailClosedSection'
import { Hero } from './sections/Hero'
import { LifecycleSection } from './sections/LifecycleSection'
import { MapSection } from './sections/MapSection'
import { OverridesSection } from './sections/OverridesSection'
import { PayoffSection } from './sections/PayoffSection'
import { WhySection } from './sections/WhySection'
import { SPEC_DOCS } from './spec-docs'

/**
 * The ordered home-page sections. The first is the hero; the rest each carry a
 * DOM id matching a NAV entry in content/deck.ts for scroll-spy.
 */
const SECTIONS: ComponentType[] = [
  Hero,
  WhySection,
  LifecycleSection,
  MapSection,
  AccessSection,
  FailClosedSection,
  OverridesSection,
  CoexistenceSection,
  PayoffSection,
]

/** deep-dive pages, keyed by the `#/<slug>` route slug. */
const Spec = () => <SpecPage docs={SPEC_DOCS} />

const PAGES: Record<string, ComponentType> = {
  'engine-overrides': EngineOverridesPage,
  'rbac-contract': RbacContractPage,
  spec: Spec,
}

function Home() {
  return (
    <main>
      {SECTIONS.map((Component, i) => (
        <Component key={i} />
      ))}
    </main>
  )
}

function NotFound() {
  return (
    <main className="px-4 py-24 @3xl:px-9">
      <p className="font-mono text-[14px] lowercase text-ink-faint">
        nothing here.{' '}
        <a href="#/" className="text-ink hover:text-accent transition-colors">
          ← back to the overview
        </a>
      </p>
    </main>
  )
}

export default function App() {
  const route = useHashRoute()
  const Page = route.kind === 'page' ? PAGES[route.slug] : undefined

  return (
    <div className="@container min-h-screen">
      <Sheet>
        <TopNav route={route} meta={DECK_META} nav={NAV} />
        {route.kind === 'home' ? <Home /> : Page ? <Page /> : <NotFound />}
        <Footer footer={FOOTER} />
      </Sheet>
    </div>
  )
}
