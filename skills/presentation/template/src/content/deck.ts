/**
 * deck.ts — the one file that describes THIS presentation.
 *
 * The /presentation skill rewrites this per tech-spec. Everything here is pure
 * data (no JSX); the shared chrome (TopNav, Footer) receives it as props from
 * App.tsx — shared components never reach into deck content. The ordered list
 * of section COMPONENTS and the deep-dive PAGES map live in App.tsx.
 */

import type { DeckMeta, FooterSpec, NavItem } from '@lib/lib/deck-types'

export const DECK_META: DeckMeta = {
  wordmarkLabel: 'spec presentation',
}

/**
 * top-nav section links. each id must match the `id` passed to a <Section>.
 * keep this short (6–9 items) — it is the reader's map of the whole deck.
 */
export const NAV: NavItem[] = [
  { id: 'map', label: 'map' },
  { id: 'payoff', label: 'payoff' },
]

export const FOOTER: FooterSpec = {
  eyebrow: 'get started',
  headline: 'one file. one command.',
  command: 'iii up',
  attribution: 'spec presentation — architecture overview',
  source: 'source of truth: tech-specs/<your-spec>',
}
