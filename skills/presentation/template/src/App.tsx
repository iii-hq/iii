import type { ComponentType } from 'react'
import { Footer } from '@/components/Footer'
import { TopNav } from '@/components/TopNav'
import { Sheet } from '@/components/schematic/Sheet'
import { useHashRoute } from '@/hooks/useHashRoute'
import { ExamplePage } from '@/pages/ExamplePage'
import { SpecPage } from '@/pages/SpecPage'
import { GuaranteesSection } from '@/sections/GuaranteesSection'
import { Hero } from '@/sections/Hero'
import { InstallSection } from '@/sections/InstallSection'
import { LifecycleSection } from '@/sections/LifecycleSection'
import { PayoffSection } from '@/sections/PayoffSection'
import { ReactiveSection } from '@/sections/ReactiveSection'
import { SequenceSection } from '@/sections/SequenceSection'
import { SystemMapSection } from '@/sections/SystemMapSection'

/**
 * The ordered home-page sections. The first is the hero; the rest each carry a
 * DOM id that should match a NAV entry in content/deck.ts for scroll-spy.
 * The /presentation skill edits this list and the PAGES map below.
 */
const SECTIONS: ComponentType[] = [
  Hero,
  SystemMapSection,
  SequenceSection,
  LifecycleSection,
  ReactiveSection,
  GuaranteesSection,
  InstallSection,
  PayoffSection,
]

/**
 * deep-dive pages, keyed by the `#/<slug>` route slug. `spec` is the built-in
 * spec viewer (renders the spec dir's markdown); replace `example` with your
 * own deep-dive pages.
 */
const PAGES: Record<string, ComponentType> = {
  example: ExamplePage,
  spec: SpecPage,
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
        <TopNav route={route} />
        {route.kind === 'home' ? <Home /> : Page ? <Page /> : <NotFound />}
        <Footer />
      </Sheet>
    </div>
  )
}
