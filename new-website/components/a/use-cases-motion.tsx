'use client'

import { useEffect, useRef, useState } from 'react'

const TICK_MS = 50

/**
 * A playhead for one demo: milliseconds since `key` last changed, counted only while `running`. When it passes
 * `total` it calls `onEnd` once (the section moves to the next demo, or the demo loops by changing its key).
 * Not running (paused, off screen, reduced motion) freezes the playhead where it is.
 */
export function usePlayhead(running: boolean, key: string, total: number, onEnd?: () => void) {
  const [clock, setClock] = useState({ key, ms: 0 })
  const ms = clock.key === key ? clock.ms : 0
  /* The key whose end has already been reported, so each run ends exactly once. */
  const endedKey = useRef<string | null>(null)
  const endRef = useRef(onEnd)
  useEffect(() => {
    endRef.current = onEnd
  }, [onEnd])

  useEffect(() => {
    if (!running) return
    const id = window.setInterval(() => {
      setClock((previous) => ({ key, ms: (previous.key === key ? previous.ms : 0) + TICK_MS }))
    }, TICK_MS)
    return () => window.clearInterval(id)
  }, [key, running])

  useEffect(() => {
    if (ms >= total && endedKey.current !== key) {
      endedKey.current = key
      endRef.current?.()
    }
  }, [ms, total, key])

  return ms
}

/** The braille spinner from the `iii compose` progress renderer (iii-hq/iii#2263): ten frames, 80ms apart. */
const FRAMES = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'] as const

/** One ticker for every spinner on screen, so they turn in step and cost a single timer. */
export function useSpinnerFrame(spinning: boolean) {
  const [frame, setFrame] = useState(0)
  useEffect(() => {
    if (!spinning) return
    const id = window.setInterval(() => setFrame((current) => (current + 1) % FRAMES.length), 80)
    return () => window.clearInterval(id)
  }, [spinning])
  return FRAMES[frame]
}

/** Elapsed seconds the way the compose renderer prints them: 0.4s, 3.3s, 9.8s. */
export const secs = (ms: number) => `${(Math.max(0, ms) / 1000).toFixed(1)}s`
