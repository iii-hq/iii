'use client'

import { useSyncExternalStore } from 'react'

const query = '(prefers-reduced-motion: reduce)'

function subscribe(onChange: () => void) {
  const media = window.matchMedia(query)
  media.addEventListener('change', onChange)
  return () => media.removeEventListener('change', onChange)
}

const getSnapshot = () => window.matchMedia(query).matches
const getServerSnapshot = () => true

/** Responds to motion preference changes without requiring a page reload. */
export function useMotionPreference() {
  return useSyncExternalStore(subscribe, getSnapshot, getServerSnapshot)
}
