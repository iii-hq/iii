/**
 * site.ts — the gallery's repo identity, set once. Everything listed on the
 * page itself comes from `virtual:spec-manifest` (each spec README's
 * frontmatter); nothing per-spec ever lands in this file.
 */

export const SITE = {
  /** text next to the wordmark in the header */
  wordmarkLabel: 'iii / tech-specs',
  /** small-caps eyebrow above the hero title */
  heroEyebrow: 'tech-specs',
  /** the big hero line — what this collection is */
  heroTitle: 'iii tech specs, made interactive',
  heroLead:
    'every iii tech spec, published in public: the spec markdown always readable, and — when a deck exists — the architecture as a navigable map, the protocol steppable, the why argued like a product launch.',
  /** left attribution in the footer bar */
  attribution: 'iii — tech specs',
  /** right "source of truth" line in the footer bar */
  source: 'source of truth: iii/tech-specs',
}
