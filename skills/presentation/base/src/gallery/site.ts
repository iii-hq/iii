/**
 * site.ts — the gallery's repo identity, set once. Everything listed on the
 * page itself comes from `virtual:spec-manifest` (each spec README's
 * frontmatter); nothing per-spec ever lands in this file. `heroLead` is the
 * one hand-curated line: it speaks in roadmap voice (what's next, what
 * landed) without naming specs, so it never drifts as entries are added.
 */

export const SITE = {
  /** text next to the wordmark in the header */
  wordmarkLabel: '__WORDMARK_LABEL__',
  /** small-caps eyebrow above the hero title */
  heroEyebrow: 'tech-specs / roadmap',
  /** the big hero line — the roadmap framing, e.g. "what we're working on" */
  heroTitle: '__HERO_TITLE__',
  heroLead: '__HERO_LEAD__',
  /** left attribution in the footer bar */
  attribution: '__ATTRIBUTION__',
  /** right "source of truth" line in the footer bar */
  source: 'source of truth: __REPO__/tech-specs',
}
