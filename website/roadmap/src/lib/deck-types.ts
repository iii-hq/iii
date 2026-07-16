/**
 * deck-types.ts — the shapes a deck's content layer feeds into the shared
 * chrome (TopNav, Footer). Each deck defines its values in its own
 * `src/content/deck.ts` and passes them as props from its App.tsx; the shared
 * components never reach into deck content.
 */

export interface NavItem {
  /** must match a section's DOM id so scroll-spy + anchor links work */
  id: string
  label: string
}

export interface FooterSpec {
  /** small caps eyebrow above the closing line, e.g. "get started" */
  eyebrow: string
  /** the big closing line — the one-command payoff */
  headline: string
  /** the command shown in the bordered chip */
  command: string
  /** left attribution line in the bottom bar */
  attribution: string
  /** right "source of truth" line in the bottom bar */
  source: string
}

export interface DeckMeta {
  /** wordmark text next to the logo in the top nav */
  wordmarkLabel: string
}
