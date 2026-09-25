import { DeckShell } from "@lib/components/DeckShell"
import { useHashRoute } from "@lib/hooks/useHashRoute"
import { SpecPage } from "@lib/pages/SpecPage"
import type { ComponentType } from "react"
import { NAV } from "./content/deck"
import { CrashRestartPage } from "./pages/CrashRestartPage"
import { CliSection } from "./sections/CliSection"
import { ConfigSection } from "./sections/ConfigSection"
import { FailureSection } from "./sections/FailureSection"
import { Hero } from "./sections/Hero"
import { MapSection } from "./sections/MapSection"
import { NamespaceSection } from "./sections/NamespaceSection"
import { OrderingSection } from "./sections/OrderingSection"
import { PayoffSection } from "./sections/PayoffSection"
import { RunItSection } from "./sections/RunItSection"
import { ScriptsSection } from "./sections/ScriptsSection"
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
      <MapSection />
      <NamespaceSection />
      <ScriptsSection />
      <ConfigSection />
      <CliSection />
      <OrderingSection />
      <FailureSection />
      <PayoffSection />
    </main>
  )
}

// The built-in spec viewer — never remove. It renders every markdown file of
// the paired tech-specs/<slug>/ directory with a file sidebar.
const Spec = () => <SpecPage docs={SPEC_DOCS} />

/** deep-dive pages, keyed by the `#/<slug>` route slug. */
const PAGES: Record<string, ComponentType> = {
  "crash-restart": CrashRestartPage,
  spec: Spec,
}

export default function App() {
  const route = useHashRoute()
  return <DeckShell route={route} nav={NAV} pages={PAGES} home={<Home />} />
}
