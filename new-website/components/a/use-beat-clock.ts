'use client'

import { useEffect, useState } from 'react'

const TICK_MS = 40

/**
 * Milliseconds since the current story beat started, counted only while the chapter is running, so a beat can play
 * its own small timeline (type a query, then walk a list). When the chapter is not running (paused, holding, off
 * screen, reduced motion) it reports Infinity: every timeline shows its finished state instead of a frozen half-step.
 */
export function useBeatClock(beat: number, running: boolean) {
  const [clock, setClock] = useState({ beat, ms: 0 })
  const ms = clock.beat === beat ? clock.ms : 0
  useEffect(() => {
    if (!running) return
    const id = window.setInterval(() => {
      setClock((previous) => ({ beat, ms: (previous.beat === beat ? previous.ms : 0) + TICK_MS }))
    }, TICK_MS)
    return () => window.clearInterval(id)
  }, [beat, running])
  return running ? ms : Number.POSITIVE_INFINITY
}

/** Characters of `text` typed after `ms`, at `perChar` milliseconds each. */
export const typed = (text: string, ms: number, perChar = 30) => text.slice(0, Math.max(0, Math.floor(ms / perChar)))
