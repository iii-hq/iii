"use client"

import { useSyncExternalStore } from "react"

function subscribe(onChange: () => void) {
  document.addEventListener("visibilitychange", onChange)
  return () => document.removeEventListener("visibilitychange", onChange)
}

/**
 * Whether the tab is in the foreground. Looping or autoplaying content pauses
 * while this is false. Hydration-safe: the server snapshot is `true`.
 */
export function usePageVisible() {
  return useSyncExternalStore(
    subscribe,
    () => document.visibilityState === "visible",
    () => true,
  )
}
