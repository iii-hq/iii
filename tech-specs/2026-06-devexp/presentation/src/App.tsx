import { Footer } from '@/components/Footer'
import { TopNav } from '@/components/TopNav'
import { Sheet } from '@/components/schematic/Sheet'
import { useHashRoute } from '@/hooks/useHashRoute'
import { ComposePage } from '@/pages/ComposePage'
import { MigrationPage } from '@/pages/MigrationPage'
import { PlaygroundPage } from '@/pages/PlaygroundPage'
import { ComposeSection } from '@/sections/ComposeSection'
import { ConfigSection } from '@/sections/ConfigSection'
import { DaemonSection } from '@/sections/DaemonSection'
import { Hero } from '@/sections/Hero'
import { MeetingPointsSection } from '@/sections/MeetingPointsSection'
import { PayoffSection } from '@/sections/PayoffSection'
import { PlaygroundSection } from '@/sections/PlaygroundSection'
import { ReadinessSection } from '@/sections/ReadinessSection'
import { RemovedSection } from '@/sections/RemovedSection'
import { SystemMapSection } from '@/sections/SystemMapSection'
import { WhySection } from '@/sections/WhySection'

function Home() {
  return (
    <main>
      <Hero />
      <WhySection />
      <PlaygroundSection />
      <SystemMapSection />
      <MeetingPointsSection />
      <DaemonSection />
      <ReadinessSection />
      <ComposeSection />
      <ConfigSection />
      <RemovedSection />
      <PayoffSection />
    </main>
  )
}

export default function App() {
  const route = useHashRoute()

  return (
    <div className="@container min-h-screen">
      <Sheet>
        <TopNav route={route} />
        {route.kind === 'home' ? (
          <Home />
        ) : route.id === 'playground' ? (
          <PlaygroundPage />
        ) : route.id === 'compose' ? (
          <ComposePage />
        ) : (
          <MigrationPage />
        )}
        <Footer />
      </Sheet>
    </div>
  )
}
