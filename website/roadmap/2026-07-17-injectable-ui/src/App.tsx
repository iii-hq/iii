import { DeckShell } from "@lib/components/DeckShell"
import { useHashRoute } from "@lib/hooks/useHashRoute"
import { SpecPage } from "@lib/pages/SpecPage"
import type { ComponentType } from "react"
import { NAV } from "./content/deck"
import { AuthoringPage } from "./pages/AuthoringPage"
import { Hero } from "./sections/Hero"
import { HotEditSection } from "./sections/HotEditSection"
import { LifecycleSection } from "./sections/LifecycleSection"
import { LimitsSection } from "./sections/LimitsSection"
import { MapSection } from "./sections/MapSection"
import { PayoffSection } from "./sections/PayoffSection"
import { ReactSection } from "./sections/ReactSection"
import { SlotsSection } from "./sections/SlotsSection"
import { StylingSection } from "./sections/StylingSection"
import { WhySection } from "./sections/WhySection"
import { WireSection } from "./sections/WireSection"
import { SPEC_DOCS } from "./spec-docs"

/**
 * The ordered home-page sections. The first is the hero; the rest each carry a
 * DOM id matching a NAV entry in content/deck.ts for scroll-spy.
 */
function Home() {
  return (
    <main>
      <Hero />
      <WhySection />
      <HotEditSection />
      <MapSection />
      <WireSection />
      <SlotsSection />
      <ReactSection />
      <StylingSection />
      <LifecycleSection />
      <LimitsSection />
      <PayoffSection />
    </main>
  )
}

// The built-in spec viewer — never remove. It renders every markdown file of
// the paired tech-specs/<slug>/ directory with a file sidebar.
const Spec = () => <SpecPage docs={SPEC_DOCS} />

/** deep-dive pages, keyed by the `#/<slug>` route slug. */
const PAGES: Record<string, ComponentType> = {
  authoring: AuthoringPage,
  spec: Spec,
}

export default function App() {
  const route = useHashRoute()
  return <DeckShell route={route} nav={NAV} pages={PAGES} home={<Home />} />
}
