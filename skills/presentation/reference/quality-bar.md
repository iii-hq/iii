# Quality bar

Run this before declaring a deck done. It maps directly to the four
requirements. Anything red gets fixed, not noted.

## Build integrity

- [ ] `pnpm typecheck` passes (strict, zero errors).
- [ ] `pnpm build` succeeds; `dist/` uses relative paths (`base: './'`) and
      opens from `file://` and any subpath.
- [ ] Dev server boots; **zero console errors/warnings** in `/browse`.
- [ ] Nothing under `template/` was modified except the content layer
      (`content/`, `sections/`, `pages/`, `App.tsx` registry, `index.html`
      title/description, any new bespoke `diagrams/`). The design system,
      hooks, primitives, and shared archetypes are untouched.
- [ ] `src/content/example.ts` and the starter example sections/pages were
      deleted or replaced — no placeholder content ships.

## Req 1 — engineers understand the spec

- [ ] Every major design pillar in the spec has a slide; nothing load-bearing
      is dropped.
- [ ] Each slide makes exactly one claim, and its visual proves it.
- [ ] The architecture is one navigable map (A4), not prose.
- [ ] Field-level detail exists for skeptics, behind a `<SpecSheet>`, closed.
- [ ] Every claim is grounded in the spec; one honest trade-offs beat is kept.

## Req 2 — interactive

- [ ] At least three genuinely interactive archetypes are present (e.g. a
      stepper, the selectable map, a toggle or decision flow).
- [ ] Every control is keyboard-operable, disables at its bounds, and shows a
      visible active state.
- [ ] All motion respects `prefers-reduced-motion`; the static fallback is
      fully readable.
- [ ] Scroll-spy nav + scroll-reveal work; deep-dive hash routes load and the
      back link returns home.

## Req 3 — looks like marketing

- [ ] The hero states the *win* in one line + three values — not the mechanism.
- [ ] A problem→solution arc is present, with at least one before/after and one
      quantified scorecard.
- [ ] The accent is rationed (success / active / CTA only); no gradient or
      shadow drift (only `deal-shadow`).
- [ ] Light and dark are both correct; the accent swaps orange↔blue; there is
      no theme flash on load.
- [ ] Typography law holds: Chivo Mono, ligatures off, lowercase copy,
      identifiers keep their casing, caps tracking on labels.
- [ ] Voice: no em-dashes; no "robust / comprehensive / crucial / seamless /
      unlock / delve".

## Req 4 — build-in-public ready

- [ ] Responsive: no horizontal body scroll at 375px; wide diagrams scroll
      inside their own `overflow-x-auto` container.
- [ ] `README.md` explains dev / build / preview and that `dist/` is a portable
      static site (any CDN / Pages / S3 / `file://`).
- [ ] Title + meta description set (`index.html` tokens substituted); the tab
      wordmark is coherent.
- [ ] No secrets, no internal-only URLs, no broken links. Safe to share.

## Anti-slop check

None of these slipped in: a gradient hero; emoji as iconography; everything
centered; three identical rounded-2xl drop-shadow feature cards; purple→blue
gradients; "Empower / Seamless / Unlock" copy; the accent on every element.
