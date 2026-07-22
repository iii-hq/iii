import { Sheet } from '@lib/components/schematic/Sheet'
import { TopNav } from '@lib/components/TopNav'
import { useHashRoute } from '@lib/hooks/useHashRoute'
import { SpecPage } from '@lib/pages/SpecPage'
import type { ComponentType } from 'react'
import { NAV } from './content/deck'
import { ProtocolPage } from './pages/ProtocolPage'
import { QualityPage } from './pages/QualityPage'
import { ScenariosPage } from './pages/ScenariosPage'
import { EvidenceSection } from './sections/EvidenceSection'
import { Hero } from './sections/Hero'
import { MapSection } from './sections/MapSection'
import { PayoffSection } from './sections/PayoffSection'
import { ReadinessSection } from './sections/ReadinessSection'
import { RouterContractSection } from './sections/RouterContractSection'
import { RunItSection } from './sections/RunItSection'
import { RunSequenceSection } from './sections/RunSequenceSection'
import { ScenariosSection } from './sections/ScenariosSection'
import { TransitionSection } from './sections/TransitionSection'
import { VerdictsSection } from './sections/VerdictsSection'
import { WhySection } from './sections/WhySection'
import { SPEC_DOCS } from './spec-docs'

const SECTIONS: ComponentType[] = [
  Hero,
  WhySection,
  RunItSection,
  MapSection,
  RunSequenceSection,
  ReadinessSection,
  RouterContractSection,
  EvidenceSection,
  ScenariosSection,
  VerdictsSection,
  TransitionSection,
  PayoffSection,
]

const Spec = () => <SpecPage docs={SPEC_DOCS} />

const PAGES: Record<string, ComponentType> = {
  protocol: ProtocolPage,
  quality: QualityPage,
  scenarios: ScenariosPage,
  spec: Spec,
}

function Home() {
  return (
    <main>
      {SECTIONS.map((Component, index) => (
        <Component key={index} />
      ))}
    </main>
  )
}

function NotFound() {
  return (
    <main className="px-4 py-24 @3xl:px-9">
      <p className="font-mono text-[14px] lowercase text-ink-faint">
        nothing here.{' '}
        <a href="#/" className="text-ink transition-colors hover:text-accent">
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
