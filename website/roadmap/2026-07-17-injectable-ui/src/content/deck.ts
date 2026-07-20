/**
 * deck.ts — the one file that describes THIS presentation.
 *
 * Nav, wordmark, footer for the injectable-ui deck. The ordered SECTIONS array
 * and the deep-dive PAGES map live in App.tsx.
 */

export interface NavItem {
  /** must match a section's DOM id so scroll-spy + anchor links work */
  id: string
  label: string
}

export interface FooterSpec {
  eyebrow: string
  headline: string
  command: string
  attribution: string
  source: string
}

export interface DeckMeta {
  wordmarkLabel: string
}

export const DECK_META: DeckMeta = {
  wordmarkLabel: 'injectable-ui',
}

/** top-nav section links — each id matches the `id` passed to a <Section>. */
export const NAV: NavItem[] = [
  { id: 'why', label: 'why' },
  { id: 'hot-edit', label: 'hot edit' },
  { id: 'map', label: 'map' },
  { id: 'wire', label: 'wire' },
  { id: 'slots', label: 'slots' },
  { id: 'one-react', label: 'react' },
  { id: 'styling', label: 'css' },
  { id: 'lifecycle', label: 'lifecycle' },
  { id: 'limits', label: 'limits' },
  { id: 'payoff', label: 'payoff' },
]

export const FOOTER: FooterSpec = {
  eyebrow: 'where it lives',
  headline: 'ui ships with the worker. the console just renders it.',
  command: 'iii worker add console',
  attribution: 'injectable console ui · runtime slots over iii primitives',
  source: 'source: iii/tech-specs/2026-07-17-injectable-ui',
}
