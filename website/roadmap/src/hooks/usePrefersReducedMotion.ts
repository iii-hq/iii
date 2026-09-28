import { useSyncExternalStore } from "react"

const QUERY = "(prefers-reduced-motion: reduce)"

function subscribe(onChange: () => void): () => void {
  const media = window.matchMedia(QUERY)
  media.addEventListener("change", onChange)
  return () => media.removeEventListener("change", onChange)
}

function getSnapshot(): boolean {
  return window.matchMedia(QUERY).matches
}

// the server has no media queries; animate by default and let the first
// client render (where the decks actually mount) read the real preference.
function getServerSnapshot(): boolean {
  return false
}

/**
 * `prefers-reduced-motion: reduce`, read as an external store so render never
 * touches `window` directly and the value tracks the OS setting live. The
 * diagram archetypes gate their ambient animations (marching dots, fade-rise)
 * on it.
 */
export function usePrefersReducedMotion(): boolean {
  return useSyncExternalStore(subscribe, getSnapshot, getServerSnapshot)
}
