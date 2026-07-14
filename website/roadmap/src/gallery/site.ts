/**
 * site.ts — the gallery's repo identity, set once. Everything listed on the
 * page itself comes from `virtual:spec-manifest` (each spec README's
 * frontmatter); nothing per-spec ever lands in this file. `heroLead` is the
 * one hand-curated line: it speaks in roadmap voice (what's next, what
 * landed) without naming specs, so it never drifts as entries are added.
 */

export const SITE = {
  /** small-caps eyebrow above the hero title */
  heroEyebrow: 'roadmap',
  /** the big hero line — what this collection is */
  heroTitle: "what we're working on",
  heroLead:
    'the iii roadmap, in public: every priority lands here as a tech spec before it lands as code. newest first, so the top entry is what we are building right now; everything below it has already shipped into the engine and its workers. each spec stays readable as markdown, and the big ones earn an interactive deck.',
}
