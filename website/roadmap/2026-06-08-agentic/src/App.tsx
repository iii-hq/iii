import { DeckShell } from "@lib/components/DeckShell"
import { useHashRoute } from "@lib/hooks/useHashRoute"
import { SpecPage } from "@lib/pages/SpecPage"
import type { ComponentType } from "react"
import { NAV } from "./content/deck"
import { ConsolePage } from "./pages/ConsolePage"
import { LoopsPage } from "./pages/LoopsPage"
import { TelegramPage } from "./pages/TelegramPage"
import { DurableSection } from "./sections/DurableSection"
import { GovernanceSection } from "./sections/GovernanceSection"
import { Hero } from "./sections/Hero"
import { OnboardingSection } from "./sections/OnboardingSection"
import { ReactiveSection } from "./sections/ReactiveSection"
import { SolvesSection } from "./sections/SolvesSection"
import { SpawnSection } from "./sections/SpawnSection"
import { SubstrateSection } from "./sections/SubstrateSection"
import { SystemMapSection } from "./sections/SystemMapSection"
import { TurnSection } from "./sections/TurnSection"
import { UseCasesSection } from "./sections/UseCasesSection"
import { SPEC_DOCS } from "./spec-docs"

/**
 * Ordered home-page sections. The first is the hero; the rest each carry a DOM
 * id matching a NAV entry in content/deck.ts for scroll-spy.
 */
function Home() {
  return (
    <main>
      <Hero />
      <SystemMapSection />
      <TurnSection />
      <SubstrateSection />
      <ReactiveSection />
      <DurableSection />
      <SpawnSection />
      <GovernanceSection />
      <UseCasesSection />
      <OnboardingSection />
      <SolvesSection />
    </main>
  )
}

const Spec = () => <SpecPage docs={SPEC_DOCS} />

/** deep-dive pages, keyed by the `#/<slug>` route slug. */
const PAGES: Record<string, ComponentType> = {
  telegram: TelegramPage,
  console: ConsolePage,
  loops: LoopsPage,
  spec: Spec,
}

export default function App() {
  const route = useHashRoute()
  return <DeckShell route={route} nav={NAV} pages={PAGES} home={<Home />} />
}
