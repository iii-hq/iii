import type { PointerEvent } from 'react'

/**
 * Pixel-heading spotlight: the dots dim on hover and a lit copy shows through a soft circle that follows
 * the pointer. The host element gets `group/title relative`, the visible text gets `spotlightBaseClass`,
 * and an `aria-hidden` copy of the same text gets `spotlightLayerClass`. Radius comes from `--spot-r`.
 */
export function spotlightMove(e: PointerEvent<HTMLElement>) {
  if (e.pointerType !== 'mouse') return
  const rect = e.currentTarget.getBoundingClientRect()
  e.currentTarget.style.setProperty('--spot-x', `${e.clientX - rect.left}px`)
  e.currentTarget.style.setProperty('--spot-y', `${e.clientY - rect.top}px`)
}

export const spotlightBaseClass =
  'transition-opacity duration-300 ease-out group-hover/title:opacity-40 motion-reduce:transition-none'

export const spotlightLayerClass =
  'pointer-events-none absolute inset-0 select-none opacity-0 transition-opacity duration-300 ease-out [mask-image:radial-gradient(var(--spot-r,150px)_circle_at_var(--spot-x,50%)_var(--spot-y,50%),#000_20%,transparent_75%)] group-hover/title:opacity-100 motion-reduce:transition-none dark:[text-shadow:0_0_18px_oklch(1_0_0/0.35)]'
