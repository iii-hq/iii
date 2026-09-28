"use client"

import { useCallback, useSyncExternalStore } from "react"
import { DARK_QUERY, resolveTheme, THEME_COLOR, THEME_KEY, type Theme, type ThemePreference } from "@/lib/theme"

// <html> is the source of truth: `.dark` + data-theme for what's on screen,
// data-theme-pref for what the visitor chose. The boot script sets them before
// first paint; this hook observes them, so every control stays in sync.

function readPreference(): ThemePreference {
  const p = document.documentElement.dataset.themePref
  return p === "light" || p === "system" ? p : "dark"
}

/** Apply a preference to <html>, with transitions frozen for the swap. */
function apply(pref: ThemePreference) {
  const root = document.documentElement
  const resolved = resolveTheme(pref)
  // Every component has its own transition timing; letting them all animate
  // the color change at once looks broken. Freeze, flush, release next frame.
  const freeze = document.createElement("style")
  freeze.textContent = "*,*::before,*::after{transition:none!important}"
  document.head.appendChild(freeze)
  root.classList.toggle("dark", resolved === "dark")
  root.dataset.theme = resolved
  root.dataset.themePref = pref
  document.querySelector('meta[name="theme-color"]')?.setAttribute("content", THEME_COLOR[resolved])
  void window.getComputedStyle(root).opacity
  requestAnimationFrame(() => freeze.remove())
}

// One observer and one media listener for the whole page, however many
// controls and canvases read the theme; each consumer only adds a listener.
const listeners = new Set<() => void>()
let stop: (() => void) | null = null

function start() {
  const notify = () => {
    for (const l of listeners) l()
  }
  const observer = new MutationObserver(notify)
  observer.observe(document.documentElement, { attributes: true, attributeFilter: ["class", "data-theme-pref"] })
  // Following the OS: re-apply when it flips.
  const media = matchMedia(DARK_QUERY)
  const onMedia = () => {
    if (readPreference() === "system") apply("system")
  }
  media.addEventListener("change", onMedia)
  return () => {
    observer.disconnect()
    media.removeEventListener("change", onMedia)
  }
}

function subscribe(onChange: () => void) {
  listeners.add(onChange)
  stop ??= start()
  return () => {
    listeners.delete(onChange)
    if (!listeners.size && stop) {
      stop()
      stop = null
    }
  }
}

const getTheme = (): Theme => (document.documentElement.classList.contains("dark") ? "dark" : "light")
const serverTheme = (): Theme => "dark"
const serverPreference = (): ThemePreference => "dark"

export function useTheme() {
  const theme = useSyncExternalStore(subscribe, getTheme, serverTheme)
  const preference = useSyncExternalStore(subscribe, readPreference, serverPreference)

  const setPreference = useCallback((next: ThemePreference) => {
    apply(next)
    try {
      localStorage.setItem(THEME_KEY, next)
    } catch {
      // storage unavailable: the theme still applies for this page view
    }
  }, [])

  const toggle = useCallback(() => setPreference(getTheme() === "dark" ? "light" : "dark"), [setPreference])

  return { theme, preference, setPreference, setTheme: setPreference, toggle }
}
