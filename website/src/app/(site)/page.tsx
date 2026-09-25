import type { Metadata } from "next"
import { ConsoleLive } from "@/components/landing/console-live"
import { Experience } from "@/components/landing/experience"
import { GetStarted } from "@/components/landing/get-started"
import { HarnessRace } from "@/components/landing/harness-race"
import { HelloWorld } from "@/components/landing/hello-world"
import { Hero } from "@/components/landing/hero"
import { Nutshell } from "@/components/landing/nutshell"
import { RoadmapPreview } from "@/components/landing/roadmap-preview"
import { Workers } from "@/components/landing/workers"
import { landingJsonLd } from "@/lib/json-ld"

// The apex canonical, so the landing page is never described by a query or a mirror.
export const metadata: Metadata = { alternates: { canonical: "/" } }

export default function Home() {
  return (
    <>
      <main>
        <Hero />
        <ConsoleLive />
        <HarnessRace />
        <Experience />
        <HelloWorld />
        <Workers />
        <Nutshell />
        <RoadmapPreview />
        <GetStarted />
      </main>
      <script
        type="application/ld+json"
        // biome-ignore lint/security/noDangerouslySetInnerHtml: JSON-LD, escaped below
        dangerouslySetInnerHTML={{ __html: JSON.stringify(landingJsonLd).replace(/</g, "\\u003c") }}
      />
    </>
  )
}
