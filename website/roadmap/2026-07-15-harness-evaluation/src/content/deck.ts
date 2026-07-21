/**
 * deck.ts — the one file that describes THIS presentation.
 *
 * Nav for the harness-evaluation deck. The ordered SECTIONS array and the
 * deep-dive PAGES map live in App.tsx.
 */

import type { NavItem } from '@lib/lib/deck-types'

/** top-nav section links — each id matches the `id` passed to a <Section>. */
export const NAV: NavItem[] = [
  { id: 'why', label: 'why' },
  { id: 'tracks', label: 'tracks' },
  { id: 'map', label: 'map' },
  { id: 'run', label: 'a run' },
  { id: 'contract', label: 'contract' },
  { id: 'oracles', label: 'oracles' },
  { id: 'break', label: 'break it' },
  { id: 'e2e', label: 'e2e tests' },
  { id: 'verdicts', label: 'verdicts' },
  { id: 'payoff', label: 'payoff' },
]
