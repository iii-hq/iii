import { useCallback, useEffect, useState, useSyncExternalStore } from "react"

type StepperOptions = {
  count: number
  /** ms to hold step `index` before autoplay advances. Keep it referentially stable. */
  dwell: (index: number) => number
  /** autoplay only ticks while this is true (e.g. the section is on screen) */
  active: boolean
  /** play by default until the user picks play/pause themselves */
  autoplay: boolean
}

/**
 * A looping step sequencer: autoplay with per-step dwell, play/pause, prev/next,
 * seek. Any manual navigation pauses it, like the original flow controls.
 */
export function useStepper({ count, dwell, active, autoplay }: StepperOptions) {
  const [index, setIndex] = useState(0)
  // null = the user hasn't chosen, follow `autoplay`
  const [choice, setChoice] = useState<boolean | null>(null)
  const playing = choice ?? autoplay

  useEffect(() => {
    if (!playing || !active) return
    const timer = window.setTimeout(() => setIndex((i) => (i + 1) % count), dwell(index))
    return () => window.clearTimeout(timer)
  }, [playing, active, index, count, dwell])

  const goTo = useCallback(
    (i: number) => {
      setChoice(false)
      setIndex(((i % count) + count) % count)
    },
    [count],
  )
  const next = useCallback(() => {
    setChoice(false)
    setIndex((i) => (i + 1) % count)
  }, [count])
  const prev = useCallback(() => {
    setChoice(false)
    setIndex((i) => (i - 1 + count) % count)
  }, [count])
  const pause = useCallback(() => setChoice(false), [])
  const toggle = useCallback(() => setChoice(!playing), [playing])

  return { index, playing, goTo, next, prev, pause, toggle }
}

const REDUCED_QUERY = "(prefers-reduced-motion: reduce)"

function subscribeReduced(onChange: () => void) {
  const mql = window.matchMedia(REDUCED_QUERY)
  mql.addEventListener("change", onChange)
  return () => mql.removeEventListener("change", onChange)
}

/**
 * prefers-reduced-motion that is hydration-safe: the server snapshot is `false`
 * and React re-renders after hydration if the client disagrees. (Motion's
 * useReducedMotion reads the media query during the first client render, which
 * would mismatch text that depends on it, like the play/pause label.)
 */
export function usePrefersReducedMotion() {
  return useSyncExternalStore(
    subscribeReduced,
    () => window.matchMedia(REDUCED_QUERY).matches,
    () => false,
  )
}
