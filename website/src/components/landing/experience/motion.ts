/** Shared enter curve for everything in the transcript (Emil's strong ease-out). */
export const EASE_OUT = [0.23, 1, 0.32, 1] as const

/** Enter as a step lands in the transcript: opacity plus a 6px rise, 250ms. */
export const STEP_ENTER = {
  initial: { opacity: 0, y: 6 },
  animate: { opacity: 1, y: 0 },
  transition: { duration: 0.25, ease: EASE_OUT },
} as const
