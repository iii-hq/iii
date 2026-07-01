/**
 * deck.ts — the one file that describes THIS presentation.
 *
 * Nav, wordmark, footer for the rbac-proxy deck. The ordered SECTIONS array and
 * the deep-dive PAGES map live in App.tsx.
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
  wordmarkLabel: 'rbac-proxy',
}

/** top-nav section links — each id matches the `id` passed to a <Section>. */
export const NAV: NavItem[] = [
  { id: 'why', label: 'why' },
  { id: 'lifecycle', label: 'lifecycle' },
  { id: 'map', label: 'map' },
  { id: 'access', label: 'access' },
  { id: 'fail-closed', label: 'fail closed' },
  { id: 'overrides', label: 'discovery' },
  { id: 'coexistence', label: 'coexist' },
  { id: 'payoff', label: 'payoff' },
]

export const FOOTER: FooterSpec = {
  eyebrow: 'where it lives',
  headline: 'one boundary. its own port. the engine, untouched.',
  command: 'iii worker add rbac-proxy',
  attribution: 'rbac-proxy · rbac at the edge',
  source: 'source: workers/tech-specs/2026-06-rbac-proxy-worker',
}
