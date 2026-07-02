import { Footer } from '@lib/components/Footer'
import { Sheet } from '@lib/components/schematic/Sheet'
import { TopNav } from '@lib/components/TopNav'
import { useHashRoute } from '@lib/hooks/useHashRoute'
import { SpecPage } from '@lib/pages/SpecPage'
import type { ComponentType } from 'react'
import { DECK_META, FOOTER, NAV } from './content/deck'
import { ConsolePage } from './pages/ConsolePage'
import { LoopsPage } from './pages/LoopsPage'
import { TelegramPage } from './pages/TelegramPage'
import { DurableSection } from './sections/DurableSection'
import { GovernanceSection } from './sections/GovernanceSection'
import { Hero } from './sections/Hero'
import { OnboardingSection } from './sections/OnboardingSection'
import { ReactiveSection } from './sections/ReactiveSection'
import { SolvesSection } from './sections/SolvesSection'
import { SpawnSection } from './sections/SpawnSection'
import { SubstrateSection } from './sections/SubstrateSection'
import { SystemMapSection } from './sections/SystemMapSection'
import { TurnSection } from './sections/TurnSection'
import { UseCasesSection } from './sections/UseCasesSection'
import { SPEC_DOCS } from './spec-docs'

/**
 * Ordered home-page sections. The first is the hero; the rest each carry a DOM
 * id matching a NAV entry in content/deck.ts for scroll-spy.
 */
const SECTIONS: ComponentType[] = [
  Hero,
  SystemMapSection,
  TurnSection,
  SubstrateSection,
  ReactiveSection,
  DurableSection,
  SpawnSection,
  GovernanceSection,
  UseCasesSection,
  OnboardingSection,
  SolvesSection,
]

const Spec = () => <SpecPage docs={SPEC_DOCS} />

/** deep-dive pages, keyed by the `#/<slug>` route slug. */
const PAGES: Record<string, ComponentType> = {
  telegram: TelegramPage,
  console: ConsolePage,
  loops: LoopsPage,
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
