/**
 * deck.ts — the one file that describes THIS presentation. Pure data; the
 * shared chrome (TopNav, Footer) receives it as props from App.tsx.
 */

import type { DeckMeta, FooterSpec, NavItem } from '@lib/lib/deck-types'

export const DECK_META: DeckMeta = {
  wordmarkLabel: 'agentic workers',
}

export const NAV: NavItem[] = [
  { id: 'map', label: 'map' },
  { id: 'turn', label: 'turn' },
  { id: 'substrate', label: 'substrate' },
  { id: 'reactive', label: 'reactive' },
  { id: 'durable', label: 'durable' },
  { id: 'spawn', label: 'sub-agents' },
  { id: 'use-cases', label: 'use cases' },
  { id: 'install', label: 'install' },
]

export const FOOTER: FooterSpec = {
  eyebrow: 'get started',
  headline: 'the loop is one command away.',
  command: 'iii worker add harness',
  attribution: 'agentic workers — architecture overview',
  source: 'source of truth: tech-specs/2026-06-08-agentic',
}
