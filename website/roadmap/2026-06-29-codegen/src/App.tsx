import { DeckShell } from "@lib/components/DeckShell"
import { useHashRoute } from "@lib/hooks/useHashRoute"
import { SpecPage } from "@lib/pages/SpecPage"
import type { ComponentType } from "react"
import { NAV } from "./content/deck"
import { HarnessConsumerPage } from "./pages/HarnessConsumerPage"
import { HarnessSection } from "./sections/HarnessSection"
import { Hero } from "./sections/Hero"
import { LanguagesSection } from "./sections/LanguagesSection"
import { PayoffSection } from "./sections/PayoffSection"
import { RunItSection } from "./sections/RunItSection"
import { SelectSection } from "./sections/SelectSection"
import { SourceOfTruthSection } from "./sections/SourceOfTruthSection"
import { SystemMapSection } from "./sections/SystemMapSection"
import { WhySection } from "./sections/WhySection"
import { SPEC_DOCS } from "./spec-docs"

/**
 * Ordered home-page sections. The first is the hero; the rest each carry a DOM
 * id matching a NAV entry in content/deck.ts for scroll-spy.
 */
function Home() {
  return (
    <main>
      <Hero />
      <WhySection />
      <RunItSection />
      <SystemMapSection />
      <SourceOfTruthSection />
      <SelectSection />
      <LanguagesSection />
      <HarnessSection />
      <PayoffSection />
    </main>
  )
}

/** deep-dive pages, keyed by the `#/<slug>` route slug. */
const Spec = () => <SpecPage docs={SPEC_DOCS} />

const PAGES: Record<string, ComponentType> = {
  "harness-consumer": HarnessConsumerPage,
  spec: Spec,
}

export default function App() {
  const route = useHashRoute()
  return <DeckShell route={route} nav={NAV} pages={PAGES} home={<Home />} />
}
