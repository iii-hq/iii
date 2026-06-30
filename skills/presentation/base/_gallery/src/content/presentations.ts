/**
 * presentations.ts — the ONE file you fill in.
 *
 * This is the manifest the gallery renders from, and the single source of truth
 * for which spec presentations this site lists. The `/presentation` skill
 * appends an entry here every time it generates a deck; you can also edit it by
 * hand to re-order, re-word, or feature a deck.
 *
 * THE CONTRACT: every entry's `slug` MUST equal the spec directory name that
 * holds the deck (the folder under tech-specs/ that contains `presentation/`).
 * `../build.mjs` builds each `<slug>/presentation/` and copies its output to
 * `dist/<slug>/`, and each card links to `<slug>/`. Same string everywhere, or
 * the card links to a 404.
 *
 * Everything here is pure data (no JSX) so it stays the readable answer to
 * "what does this site host". The gallery chrome and cards live in components/.
 */

export interface GalleryMeta {
  /** text next to the wordmark in the header, e.g. "iii / tech-specs" */
  wordmarkLabel: string
  /** small-caps eyebrow above the hero title */
  heroEyebrow: string
  /** the big hero line — what this collection is */
  heroTitle: string
  /** one or two sentences under the hero title */
  heroLead: string
  /** left attribution in the footer bar */
  attribution: string
  /** right "source of truth" line in the footer bar */
  source: string
}

export interface Presentation {
  /** url slug — MUST equal the spec directory name (build copies dist/<slug>/) */
  slug: string
  /** deck title, lowercase, e.g. "the developer experience overhaul" */
  title: string
  /** the single claim / one-line tagline shown on the card */
  tagline: string
  /** the spec this came from, e.g. "tech-specs/2026-06-devexp" */
  spec: string
  /** date label shown on the card, e.g. "2026-06" — also the sort key */
  date: string
  /** short topic tags, e.g. ["architecture", "migration"] (0–4 read best) */
  tags?: string[]
  /** 'live' (default) shows the deck; 'draft' muted + flagged, still links */
  status?: 'live' | 'draft'
  /** pin to the top of the grid regardless of date */
  featured?: boolean
}

export const GALLERY_META: GalleryMeta = {
  wordmarkLabel: '__GALLERY_WORDMARK__',
  heroEyebrow: 'tech-specs',
  heroTitle: '__GALLERY_HERO_TITLE__',
  heroLead: '__GALLERY_HERO_LEAD__',
  attribution: '__GALLERY_ATTRIBUTION__',
  source: '__GALLERY_SOURCE__',
}

/**
 * The decks this site hosts. The `/presentation` skill keeps this in sync; the
 * example below shows the shape — replace it with real entries.
 */
export const PRESENTATIONS: Presentation[] = [
  {
    slug: '__EXAMPLE_SLUG__',
    title: '__example deck title__',
    tagline: 'the single load-bearing claim, in one lowercase line.',
    spec: 'tech-specs/__EXAMPLE_SLUG__',
    date: '2026-06',
    tags: ['architecture'],
    status: 'draft',
  },
]
