'use client'

import { useEffect, useState } from 'react'

import { CLOCK, type Stage } from './model'

/**
 * Beats for one stage of the graph. Counts 0..steps-1 on a loop while `active`; when idle it rests on the last
 * beat, which every stage draws as its complete, readable state.
 */
export function useStageClock(stage: Stage, active: boolean) {
  const { steps, ms } = CLOCK[stage]
  const [tick, setTick] = useState(0)

  // biome-ignore lint/correctness/useExhaustiveDependencies: A stage change must restart the script from its first beat.
  useEffect(() => {
    setTick(0)
    if (!active) return
    const id = window.setInterval(() => setTick((t) => t + 1), ms)
    return () => window.clearInterval(id)
  }, [active, ms, stage])

  return { step: active ? tick % steps : steps - 1, cycle: Math.floor(tick / steps), steps }
}
