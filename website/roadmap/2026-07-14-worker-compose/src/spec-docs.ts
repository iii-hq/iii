/**
 * Bundles this spec's markdown for the #/spec page. The glob MUST live
 * deck-side (import.meta.glob resolves relative to the importing file); its
 * literal is substituted at scaffold time because the relative depth from
 * <base>/<slug>/src/ to <specs-dir>/<slug>/ depends on where the base project
 * lives (in iii: '../../../../tech-specs/<slug>/*.md').
 */
export const SPEC_DOCS = import.meta.glob('../../../../tech-specs/2026-07-14-worker-compose/*.md', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>
