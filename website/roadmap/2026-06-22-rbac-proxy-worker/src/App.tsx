import { DeckShell } from "@lib/components/DeckShell"
import { useHashRoute } from "@lib/hooks/useHashRoute"
import { SpecPage } from "@lib/pages/SpecPage"
import type { ComponentType } from "react"
import { NAV } from "./content/deck"
import { EngineOverridesPage } from "./pages/EngineOverridesPage"
import { RbacContractPage } from "./pages/RbacContractPage"
import { AccessSection } from "./sections/AccessSection"
import { CoexistenceSection } from "./sections/CoexistenceSection"
import { FailClosedSection } from "./sections/FailClosedSection"
import { Hero } from "./sections/Hero"
import { LifecycleSection } from "./sections/LifecycleSection"
import { MapSection } from "./sections/MapSection"
import { OverridesSection } from "./sections/OverridesSection"
import { PayoffSection } from "./sections/PayoffSection"
import { WhySection } from "./sections/WhySection"
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
      <LifecycleSection />
      <MapSection />
      <AccessSection />
      <FailClosedSection />
      <OverridesSection />
      <CoexistenceSection />
      <PayoffSection />
    </main>
  )
}

/** deep-dive pages, keyed by the `#/<slug>` route slug. */
const Spec = () => <SpecPage docs={SPEC_DOCS} />

const PAGES: Record<string, ComponentType> = {
  "engine-overrides": EngineOverridesPage,
  "rbac-contract": RbacContractPage,
  spec: Spec,
}

export default function App() {
  const route = useHashRoute()
  return <DeckShell route={route} nav={NAV} pages={PAGES} home={<Home />} />
}
