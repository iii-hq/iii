/**
 * deck.ts - the one file that describes THIS presentation (pure data, no JSX).
 * Section components + the deep-dive PAGES map live in App.tsx.
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
  wordmarkLabel: 'iii / codegen',
}

export const NAV: NavItem[] = [
  { id: 'why', label: 'the blind spot' },
  { id: 'run', label: 'run it' },
  { id: 'map', label: 'map' },
  { id: 'discover', label: 'discover' },
  { id: 'select', label: 'select' },
  { id: 'languages', label: 'languages' },
  { id: 'harness', label: 'harness' },
  { id: 'payoff', label: 'payoff' },
]

export const FOOTER: FooterSpec = {
  eyebrow: 'get started',
  headline: 'one command. every type you need.',
  command: 'codegen generate --config codegen.yml',
  attribution: 'iii codegen · typed worker integrations',
  source: 'source of truth: workers/tech-specs/2026-06-29-codegen',
}
