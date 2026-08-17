import type { DeckMeta, FooterSpec, NavItem } from '@lib/lib/deck-types'

export const DECK_META: DeckMeta = {
  wordmarkLabel: 'harness / evaluation',
}

export const NAV: NavItem[] = [
  { id: 'why', label: 'tracks' },
  { id: 'run', label: 'operate' },
  { id: 'map', label: 'map' },
  { id: 'readiness', label: 'integration' },
  { id: 'contract', label: 'quality' },
  { id: 'evidence', label: 'evidence' },
  { id: 'scenarios', label: 'scenarios' },
  { id: 'transition', label: 'gates' },
  { id: 'payoff', label: 'payoff' },
]

export const FOOTER: FooterSpec = {
  eyebrow: 'evaluate the public path',
  headline: 'prove the system. measure the agent.',
  command: 'integration + quality / e2e',
  attribution: 'harness evaluation',
  source: 'source of truth: tech-specs/2026-07-15-harness-evaluation',
}
