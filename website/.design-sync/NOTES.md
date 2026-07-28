# design-sync notes

- 2026-07-22 first sync, project "iii Design Language" (f47a3a9f-48d0-42f7-b2e3-e462097d94a0), design-language-only scope per user choice.
- Repo is an Astro static site; the design system is CSS, so the sync is produced by hand (outside the converter envelope): styles.css + tokens + fonts + guidelines + four @dsCard preview HTMLs under components/foundations/. No _ds_bundle.js, no _ds_sync.json (un-anchored on purpose; a re-sync re-verifies everything).
- Sources of truth in the repo: light tokens in src/styles/landing.css `:root`; dark tokens live ONLY in src/components/ThemeInit.astro (inline JS, applied pre-paint). Dark accent is #3ea8ff (blue); light is #ff5a1f (orange). A re-sync must re-check BOTH files for token drift.
- Curated class vocabulary lifted verbatim: .hero-eyebrow/.hero-headline/.hero-sub, .cta-get-started(.ghost), .nut-title/.nut-group-grid/.nut-cell/.nut-meta, #nav/.logo-word, .foot*. If those rules change in src/styles/, ds-bundle/styles.css must be re-extracted.
- Preview verification: playwright is not a repo dep; install with `npm i --no-save playwright` in scratchpad and launch with `channel: 'chrome'` (system Chrome; the cached playwright browsers version-mismatch).
- User's existing "iii Design System" project (019dbfc1-...) is hand-curated and unrelated to this sync; never write to it.
- ds-bundle/ is build output, gitignored.
- 2026-07-22 user feedback: NO spec microheader labels in preview cards (e.g. "eyebrow, 12px / 500 / uppercase" captions above each sample). Show the pattern itself; specs stay in tokens.json and guidelines only.
- 2026-07-22 user feedback: the uppercase meta/figure-label strips (.nut-meta, "01 / 09" pagination markers) are banned from the design language; pattern removed from styles.css, tokens, guidelines, and previews. A "Restraint" rule now leads the conventions: minimal information only, add elements on user request, never sprinkle decorative extras.
