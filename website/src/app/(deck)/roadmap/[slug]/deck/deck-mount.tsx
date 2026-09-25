"use client"

import dynamic from "next/dynamic"

// The decks read window.location (hash routing) and localStorage at mount, so
// they render in the browser only; the static export ships the shell above.
const DeckHost = dynamic(() => import("../../../../../../roadmap/src/DeckHost"), { ssr: false })

export function DeckMount({ slug }: { slug: string }) {
  return <DeckHost slug={slug} />
}
