# Design system — the law

This is the locked visual system. It lives in the base project's
`src/index.css` (in iii: `website/roadmap/src/index.css`; seeded from
this skill's `base/`) as Tailwind v4 `@theme` tokens. **Do not change the
tokens in a deck run.** This file is the reference for *using* them correctly
in generated content.

The feel: a monospace drafting sheet. Emphasis comes from 1px borders and a
single rationed accent — never from gradients, shadows, or saturation. It
should read like a system you trust.

## Color tokens

Used as Tailwind utilities: `bg-bg`, `text-ink`, `border-rule`, `fill-accent`,
`stroke-ink-faint`, etc. Dark mode swaps automatically via
`[data-theme="dark"]`; never hardcode hex in components.

| Token | Light | Dark | Use for |
|---|---|---|---|
| `bg` | `#f2f0ed` | `#111110` | page / card background |
| `panel` | `#e9e6e2` | `#1a1916` | header strips, toggles |
| `paper-2` | `#ebe8e3` | `#1f1e1c` | a second, subtler surface |
| `ink` | `#0a0a0a` | `#f2f0ed` | primary text, strong borders |
| `ink-faint` | `#6b6865` | `#9c9893` | body copy, secondary text |
| `ink-ghost` | `#a3a09c` | `#5d5a55` | tertiary text, section numbers |
| `rule` | `#d8d5d0` | `#2a2926` | structural 1px lines |
| `rule-2` | `#e6e3df` | `#1f1e1c` | softer inner dividers |
| `accent` | `#ff5a1f` | `#3ea8ff` | success / active / CTA only |
| `accent-fg` | `#f2f0ed` | `#111110` | text on an accent fill |
| `alert` | `#c43e1c` | `#c43e1c` | errors, removed/rejected paths |
| `warn` | `#a87a00` | `#a87a00` | warnings, held states |

**Accent discipline.** The accent is the most powerful tool here precisely
because it is rare: ✓ success, the active nav link / map edge / step, a primary
CTA, a string literal in code. If it is on more than ~5% of the screen, it has
lost its meaning. Active states use an accent *border + text*, not a fill — the
only fill is the primary button.

## Radii

Only two exist: `rounded-none` (0) and `rounded-full` (pills/dots). Nothing in
between. No `rounded-lg`, no `rounded-2xl`.

## Typography

- **Font:** Chivo Mono (400/500/600), loaded in `index.html`. Set as both
  `--font-sans` and `--font-mono`. Everything is monospace.
- **Ligatures are hard-disabled** (`font-feature-settings: 'liga' 0 …`). The
  schematic look depends on monospace column alignment — do not re-enable them.
- **Scale:** hero h1 44/64px semibold, tracking `-0.02em`; section h2 28px
  medium; label-caps 11–12px uppercase, tracking `0.06–0.18em`; body/code
  12–14px, leading 1.6–1.7.
- **Case:** all copy is **lowercase** by default. The *only* exceptions are
  identifiers, function ids, type names, and code — they keep their original
  casing (use `<FnChip>` / `<CodeBlock>`). No Title Case headlines.

## Layout

- The whole deck sits in one centered `<Sheet>`: `max-w-[1200px]` with
  `border-x border-rule`.
- Responsive via **container queries** (`@2xl @3xl @4xl @5xl`), because the
  sheet — not the viewport — is the layout context. Use `@3xl:` etc., not `md:`.
- Gutters `px-4` → `@3xl:px-9`. Section verticals `py-12` → `@3xl:py-16`.
- **Visual grids are built with `gap-px bg-rule`**: lay cells in a grid with a
  1px gap over a `rule`-colored background so the rule shows through as
  hairlines. Do not draw a border on every cell.
- Wide content (SVG diagrams, tables, code) must scroll inside its own
  `overflow-x-auto` container with a `min-w-[...]`. The page body must never
  scroll horizontally.

## Motion

All keyframes live in `index.css` and are **disabled under
`prefers-reduced-motion`** centrally. Any *infinite/auto* animation a component
starts must also gate behind a runtime check:

```ts
const reducedMotion = useMemo(
  () => typeof window !== 'undefined' &&
    window.matchMedia('(prefers-reduced-motion: reduce)').matches,
  [],
)
```

| Utility | What | Where |
|---|---|---|
| `reveal` / `reveal-in` | fade + 8px rise on scroll-in | every `<Section>` |
| `pulse-dot` | expanding ring on a dot | active status indicators |
| `flow-dash` / `flow-dash-slow` | marching dashes on an edge | active wires |
| `ripple-ring` | expanding 1px ring | event fan-out source |
| `fade-rise` | line fades + rises | staggered terminal output |
| `blink` | cursor | terminal caret |
| SVG `<animateMotion>` | a 2.6px circle travels a path | the active edge/arrow |

Stagger terminal lines with inline `animationDelay: i * 70ms` (cap ~700ms).
Motion should feel calm and purposeful — one thing moves to draw the eye, never
a carousel.

## The one shadow

`deal-shadow` exists for stacked "card deck" effects only. No other shadows.
No glows. No gradients except the tiny scroll-fade masks already in
`SystemMap`'s datasheet.

## Anti-slop guardrails

The drafting-sheet system exists to prevent generic AI-site slop. Treat any
urge toward these as a bug: hero gradients; emoji as iconography; everything
centered; three identical rounded-2xl drop-shadow feature cards; purple→blue
gradients; "Empower / Seamless / Unlock" copy; an accent on every element.
