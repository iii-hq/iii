import type { Metadata } from "next"
import { notFound } from "next/navigation"
import { getAllSpecs } from "@/lib/specs"
import { DeckMount } from "./deck-mount"

type Params = { slug: string }

export const dynamicParams = false

/** Every spec with a deck, drafts included: a deck that exists gets built, as before. */
const decks = () => getAllSpecs().filter((s) => s.deckUrl)

export function generateStaticParams(): Params[] {
  return decks().map((s) => ({ slug: s.slug }))
}

export async function generateMetadata({ params }: { params: Promise<Params> }): Promise<Metadata> {
  const { slug } = await params
  const spec = decks().find((s) => s.slug === slug)
  if (!spec) return {}
  const url = `/roadmap/${spec.slug}/deck/`
  return {
    title: `${spec.title} — interactive deck`,
    description: spec.tagline,
    alternates: { canonical: url },
    icons: { icon: "/favicon.svg" },
    openGraph: { type: "article", url, title: spec.title, description: spec.tagline },
  }
}

export default async function DeckPage({ params }: { params: Promise<Params> }) {
  const { slug } = await params
  const spec = decks().find((s) => s.slug === slug)
  if (!spec) notFound()

  return (
    <>
      <div className="border-b border-rule bg-bg">
        <div className="mx-auto flex max-w-[1200px] items-center gap-x-4 px-4 py-2.5 @3xl:px-9">
          <a
            href={`/roadmap/${spec.slug}/`}
            className="min-w-0 truncate font-mono text-[12px] lowercase text-ink-faint transition-colors hover:text-ink"
          >
            ← {spec.title}
          </a>
          <span className="ml-auto shrink-0 font-mono text-[11px] uppercase tracking-[0.14em] text-ink-ghost">
            interactive deck
          </span>
        </div>
      </div>
      <DeckMount slug={spec.slug} />
    </>
  )
}
