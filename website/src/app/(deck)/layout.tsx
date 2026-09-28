import type { ReactNode } from "react"
// The decks' own Tailwind root: paper/orange palette, Chivo Mono. Not the
// site's globals.css — a deck page is a separate document.
import "../../../roadmap/src/index.css"

/**
 * Root layout for the roadmap decks (/roadmap/<slug>/deck/). The site lives
 * under (site) with its own root layout; this one is deliberately bare so the
 * decks render exactly as their standalone builds did. Title and description
 * come from the page's generateMetadata.
 */
export default function DeckLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  )
}
