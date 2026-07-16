/**
 * Bundles this spec's markdown for the #/spec page. The glob must live
 * deck-side (import.meta.glob resolves relative to the importing file) and its
 * depth is fixed by the two-tree layout:
 * website/roadmap/<slug>/src/ → ../../../../tech-specs/<slug>/
 */
export const SPEC_DOCS = import.meta.glob('../../../../tech-specs/2026-06-22-rbac-proxy-worker/*.md', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>
