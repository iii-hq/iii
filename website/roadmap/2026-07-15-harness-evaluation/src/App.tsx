import { Sheet } from '@lib/components/schematic/Sheet'
import { TopNav } from '@lib/components/TopNav'
import { useHashRoute } from '@lib/hooks/useHashRoute'
import { SpecPage } from '@lib/pages/SpecPage'
import type { ComponentType } from 'react'
import { NAV } from './content/deck'
import { AgentQualityProtocolPage } from './pages/AgentQualityProtocolPage'
import { IntegrationContractsPage } from './pages/IntegrationContractsPage'
import { IntegrationRunSection } from './sections/IntegrationRunSection'
import { ContractSection } from './sections/ContractSection'
import { Hero } from './sections/Hero'
import { LifecycleSection } from './sections/LifecycleSection'
import { MapSection } from './sections/MapSection'
import { OraclesSection } from './sections/OraclesSection'
import { PayoffSection } from './sections/PayoffSection'
import { QualityLoopSection } from './sections/QualityLoopSection'
import { TracksSection } from './sections/TracksSection'
import { VerdictsSection } from './sections/VerdictsSection'
import { WhySection } from './sections/WhySection'
import { SPEC_DOCS } from './spec-docs'

/**
 * The ordered home-page sections. The first is the hero; the rest each carry a
 * DOM id matching a NAV entry in content/deck.ts for scroll-spy.
 */
const SECTIONS: ComponentType[] = [
  Hero,
  WhySection,
  TracksSection,
  MapSection,
  IntegrationRunSection,
  LifecycleSection,
  ContractSection,
  OraclesSection,
  QualityLoopSection,
  VerdictsSection,
  PayoffSection,
]

/** deep-dive pages, keyed by the `#/<slug>` route slug. */
const Spec = () => <SpecPage docs={SPEC_DOCS} />

const PAGES: Record<string, ComponentType> = {
  'integration-contracts': IntegrationContractsPage,
  'agent-quality-protocol': AgentQualityProtocolPage,
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
        <TopNav route={route} nav={NAV} />
        {route.kind === 'home' ? <Home /> : Page ? <Page /> : <NotFound />}
      </Sheet>
    </div>
  )
}
