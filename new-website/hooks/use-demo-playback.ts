'use client'

import { useEffect, useRef, useState, useSyncExternalStore } from 'react'
import { useMotionPreference } from './use-motion-preference'

function subscribeVisibility(callback: () => void) {
  document.addEventListener('visibilitychange', callback)
  return () => document.removeEventListener('visibilitychange', callback)
}

const getVisibility = () => !document.hidden
const getServerVisibility = () => false

/** Autoplay is available only while the demo is visible and motion is allowed. */
export function useDemoPlayback<T extends Element = HTMLDivElement>() {
  const ref = useRef<T>(null)
  const [inView, setInView] = useState(false)
  const [paused, setPaused] = useState(false)
  const reduce = useMotionPreference()
  const visible = useSyncExternalStore(subscribeVisibility, getVisibility, getServerVisibility)

  useEffect(() => {
    const element = ref.current
    if (!element) return
    const observer = new IntersectionObserver(([entry]) => setInView(entry.isIntersecting), {
      rootMargin: '-10% 0px -10% 0px',
    })
    observer.observe(element)
    return () => observer.disconnect()
  }, [])

  return { ref, inView, reduce, paused, setPaused, running: inView && visible && !paused && !reduce }
}
