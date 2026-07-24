# iii design language, how to build with it

This is a design LANGUAGE sync, from a static-HTML site (iii.dev). There is no component bundle; build with generic components styled entirely by these rules. The truth lives in `styles.css` (imports `tokens/tokens.css`), `tokens/tokens.json`, and `guidelines/design-language.md`, read them before styling anything.

## Restraint (the first rule)

Place as little information on the page as is necessary to convey the goal. Do not sprinkle in decorative extras of your own: no spec captions, no uppercase meta/figure labels, no pagination markers ("01 / 09"), no ornamental annotations. If the user asks for an element, add it; otherwise leave it out.

## Setup

- One typeface for everything: `'Chivo Mono', ui-monospace, monospace`, headlines, body, UI, code. Disable ligatures (`font-variant-ligatures: none`). Fonts ship in `fonts/` and are declared by `styles.css`.
- Set `background: var(--bg); color: var(--ink)` on the page root. Every color must be a token, never hardcode hex.
- Theming: add `data-theme="dark"` on the root element to flip the whole token set. The accent CHANGES with theme: orange `#ff5a1f` in light, blue `#3ea8ff` in dark, another reason to always write `var(--accent)`.

## Token vocabulary (from `tokens/tokens.css`)

Surfaces: `--bg` (warm paper #f5f3ee), `--panel` (raised fill), `--paper`, `--paper-2`. Text: `--ink` (primary), `--ink-2`, `--ink-soft`, `--ink-faint` (secondary/body), `--ink-ghost` (faintest labels), `--mute`, `--mute-2`. Lines: `--rule` (structural 1px borders), `--rule-2` (subtler). Accent: `--accent`, used sparingly: one highlighted word, a 6px dot, a hover color.

## The idiom

- Sharp rectangles. `border-radius: 0` on everything except tiny status dots. No drop shadows, hierarchy comes from 1px `var(--rule)` lines and `var(--panel)` fills.
- Small type, generous line-height: body 13-14px at 1.7, UI 12-13px, the eyebrow kicker at 12px with wide letter-spacing (0.14em). Headlines are lowercase, tight (-0.02em), `clamp()`-sized.
- Structure is drawn: grids of cells sharing 1px `var(--rule)` borders (see `.nut-group-grid`/`.nut-cell` in `styles.css`).
- Buttons (`.cta-get-started`, `.cta-get-started.ghost`): 1px `var(--ink)` border, 13px/500, sharp corners, and an instant (.12s) ink/background inversion on hover. No accent-filled buttons.
- Every section opens with an eyebrow kicker (`.hero-eyebrow`: uppercase, letterspaced, with a 3px ink `.bar` and 6px accent `.dot`), then a lowercase headline (`.hero-headline`), then muted body copy (`.hero-sub`).
- Respect `prefers-reduced-motion`.

## Idiomatic snippet

```html
<section style="background:var(--bg); padding:96px 36px;">
  <p class="hero-eyebrow"><span class="bar"></span> the runtime <span class="dot"></span></p>
  <h1 class="hero-headline">backends for <span class="desc">agents</span>.</h1>
  <p class="hero-sub">Small muted copy, 14px, line-height 1.7, max-width 46ch.</p>
  <a class="cta-get-started" href="#">get started <span class="arrow">-&gt;</span></a>
  <a class="cta-get-started ghost" href="#">read the docs</a>
</section>
```
