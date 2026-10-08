/**
 * The site's motion vocabulary. Every Motion transition on the page draws from here so the
 * whole landing page moves with one hand. CSS counterparts live in `app/globals.css`
 * (`--motion-*` custom properties, and the `ease-out` / `ease-in-out` utilities map to them).
 */

/** Strong ease-out for anything entering, responding to input, or revealing. */
export const easeOut = [0.22, 1, 0.36, 1] as const

/** Strong ease-in-out for things that move from one place to another on screen (packets, slides). */
export const easeInOut = [0.65, 0, 0.35, 1] as const

/** Durations in seconds. Keep UI under 0.3s; graphics and reveals may run longer. */
export const duration = {
  /** Hover, press, colour: 150ms. */
  fast: 0.15,
  /** Content swaps and small enters: 220ms. */
  base: 0.22,
  /** Panels, popovers, surfaces: 300ms. */
  slow: 0.3,
  /** Section reveals on scroll: 500ms. */
  reveal: 0.5,
  /** Pixel headings printing in, left to right: 800ms. */
  print: 0.8,
} as const

/** Springs: no overshoot by default, in the Apple sense (critically damped). */
export const spring = {
  /** Indicators, thumbs, and hover pills that follow the pointer or a selection. */
  snappy: { type: 'spring', stiffness: 500, damping: 40 },
  /** Larger surfaces morphing (the floating header). */
  soft: { type: 'spring', stiffness: 380, damping: 36, mass: 0.9 },
  /** The one allowed overshoot: a thumb squashing as it lands. */
  squash: { type: 'spring', stiffness: 700, damping: 24 },
} as const

/** Stagger between siblings entering together (30–80ms per Emil Kowalski). */
export const stagger = 0.05
