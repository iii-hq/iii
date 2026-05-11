# WORK — Website

Website work. Sourced from
[`PROGRESS-repo-checks.md`](../PROGRESS-repo-checks.md).

Repo: `iii-temp/website/` (static HTML — `index.html`, `manifesto.html`).

## Survey result: aligned

The website audit found minimal docs-side work. Most items resolved or out-of-scope.
This file exists for completeness / future reference.

Confirmed:
- [x] Product framing aligned (lead language ported from website hero into
  `index.mdx`).
- [x] Install / onboarding flow alignment confirmed.
- [x] No iii vs motia drift in product naming.
- [x] URL drift resolved by switching docs paths to match website (`/install`,
  `/quickstart` at root). No redirects needed.

Skipped per user direction:
- "Motia LLC" copyright on footer — legal entity, not a branding concern.

## Active items

- [ ] **Rebrand Discord invite to `discord.gg/iiidev`.** Replace every `discord.gg/<old>`
  link (current value: `discord.gg/motia`) across the website, docs, READMEs, agent
  skill bundles, and any other repo source. Search broadly for `discord.gg` — the new
  invite is the single canonical link. Reverses the earlier "not pursuing" decision.

## Watch items (no current work)

- [ ] If the website redesigns its hero, re-port the lead language into `index.mdx`
  (or decouple — the audit's resolution was to mirror the website's hero verbatim).
- [ ] If the website adds new public pages that should be in its sitemap, update
  `iii-temp/website/sitemap.xml` (currently lists 4 URLs: `/`, `/manifesto`,
  `/llms.txt`, `/AGENTS.md`).
- [ ] If `iii.dev/docs/*` deployment configuration changes (the Vercel proxy from
  website to `iii-docs.vercel.app`), confirm docs paths still resolve correctly.

## Voice alignment

- [x] `project-rules/voice.md` captures the manifesto-aligned voice. When website prose
  changes substantially, revisit the voice rule to keep it in sync.

## Notes / dependencies

- No active work blocked on the website.
- Voice rule (already authored) keeps docs and website tonally aligned.
