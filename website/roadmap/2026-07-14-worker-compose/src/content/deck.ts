/**
 * deck.ts — the one file that describes THIS presentation.
 * Pure data; TopNav/Footer receive it from App.tsx.
 */

import type { DeckMeta, FooterSpec, NavItem } from '@lib/lib/deck-types'

export const DECK_META: DeckMeta = {
  wordmarkLabel: 'worker compose',
}

/** top-nav section links — each id matches the `id` passed to a <Section>. */
export const NAV: NavItem[] = [
  { id: 'why', label: 'why' },
  { id: 'run-it', label: 'run it' },
  { id: 'map', label: 'map' },
  { id: 'namespace', label: 'namespace' },
  { id: 'scripts', label: 'scripts' },
  { id: 'config', label: 'config' },
  { id: 'cli', label: 'contract' },
  { id: 'payoff', label: 'payoff' },
]

export const FOOTER: FooterSpec = {
  eyebrow: 'get started',
  headline: 'one file. one contract. zero zombies.',
  command: 'iii compose --id host-a --file worker-compose.yaml',
  attribution: 'worker compose · distributed worker lifecycle',
  source: 'source of truth: tech-specs/2026-07-14-worker-compose',
}
