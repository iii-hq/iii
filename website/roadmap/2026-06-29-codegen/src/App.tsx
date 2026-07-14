import { Sheet } from '@lib/components/schematic/Sheet'
import { TopNav } from '@lib/components/TopNav'
import { useHashRoute } from '@lib/hooks/useHashRoute'
import { SpecPage } from '@lib/pages/SpecPage'
import type { ComponentType } from 'react'
import { NAV } from './content/deck'
import { HarnessConsumerPage } from './pages/HarnessConsumerPage'
import { HarnessSection } from './sections/HarnessSection'
import { Hero } from './sections/Hero'
import { LanguagesSection } from './sections/LanguagesSection'
import { PayoffSection } from './sections/PayoffSection'
import { RunItSection } from './sections/RunItSection'
import { SelectSection } from './sections/SelectSection'
import { SourceOfTruthSection } from './sections/SourceOfTruthSection'
import { SystemMapSection } from './sections/SystemMapSection'
import { WhySection } from './sections/WhySection'
import { SPEC_DOCS } from './spec-docs'

/**
 * Ordered home-page sections. The first is the hero; the rest each carry a DOM
 * id matching a NAV entry in content/deck.ts for scroll-spy.
 */
const SECTIONS: ComponentType[] = [
  Hero,
  WhySection,
  RunItSection,
  SystemMapSection,
  SourceOfTruthSection,
  SelectSection,
  LanguagesSection,
  HarnessSection,
  PayoffSection,
]

/** deep-dive pages, keyed by the `#/<slug>` route slug. */
const Spec = () => <SpecPage docs={SPEC_DOCS} />

const PAGES: Record<string, ComponentType> = {
  'harness-consumer': HarnessConsumerPage,
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
