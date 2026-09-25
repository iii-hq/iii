import { useCallback, useEffect, useRef, useState } from "react"
import { track } from "@/lib/analytics"
import type { Step } from "./steps"

type SequencerState = {
  /** bumps on every (re)start; key step elements with it so a restart remounts them */
  session: number
  /** number of steps currently on screen */
  revealed: number
  /** typing dots shown while the next step's delay runs */
  typing: boolean
  finished: boolean
  /** hero-sized first message, until the ticket is clicked */
  intro: boolean
}

const INITIAL: SequencerState = { session: 0, revealed: 0, typing: false, finished: false, intro: true }
const FIRST_STEP_FALLBACK_DELAY = 250

/**
 * Timeline for the experience flow. Owns the between-step delay (typing dots),
 * the reveal cursor and the analytics events. Each step owns its own internal
 * timers and calls `advance(index)` when it is done; stale or repeated calls are ignored.
 */
export function useSequencer(steps: readonly Step[]) {
  const [state, setState] = useState<SequencerState>(INITIAL)
  // index of the step being revealed / on screen; === steps.length once finished
  const cursor = useRef(-1)
  const revealed = useRef(0)
  const delayTimer = useRef<ReturnType<typeof setTimeout> | null>(null)

  const clearDelay = useCallback(() => {
    if (delayTimer.current) clearTimeout(delayTimer.current)
    delayTimer.current = null
  }, [])

  const reveal = useCallback(
    (index: number) => {
      delayTimer.current = null
      revealed.current = index + 1
      const step = steps[index]
      track("experience_step_reveal", { step_id: step.id, step_index: index, step_type: step.type })
      setState((s) => ({ ...s, typing: false, revealed: index + 1 }))
    },
    [steps],
  )

  const next = useCallback(() => {
    const index = cursor.current + 1
    if (index > steps.length) return
    cursor.current = index
    clearDelay()
    if (index === steps.length) {
      track("experience_complete", { steps_total: steps.length })
      setState((s) => ({ ...s, typing: false, finished: true }))
      return
    }
    const delay = steps[index].delay ?? 0
    if (delay > 0) {
      setState((s) => ({ ...s, typing: true }))
      delayTimer.current = setTimeout(() => reveal(index), delay)
    } else {
      reveal(index)
    }
  }, [steps, clearDelay, reveal])

  const start = useCallback(() => {
    if (!steps.length) return
    clearDelay()
    cursor.current = 0
    revealed.current = 0
    setState((s) => ({ session: s.session + 1, revealed: 0, typing: true, finished: false, intro: true }))
    delayTimer.current = setTimeout(() => reveal(0), steps[0].delay || FIRST_STEP_FALLBACK_DELAY)
  }, [steps, clearDelay, reveal])

  /** Called by step `index` when it is done. `label` marks a user-driven (button) advance. */
  const advance = useCallback(
    (index: number, label?: string) => {
      if (index !== cursor.current) return
      if (label !== undefined) {
        const step = steps[index]
        track("experience_step_advance", {
          step_id: step.id,
          step_index: index,
          step_type: step.type,
          advance_method: "button",
          button_label: label.trim().toLowerCase(),
        })
      }
      next()
    },
    [steps, next],
  )

  const restart = useCallback(() => {
    const reached = revealed.current - 1
    track("experience_restart", { reached_step_index: reached, reached_step_id: steps[reached]?.id })
    start()
  }, [steps, start])

  const endIntro = useCallback(() => setState((s) => (s.intro ? { ...s, intro: false } : s)), [])

  useEffect(() => clearDelay, [clearDelay])

  return { ...state, start, restart, advance, endIntro }
}

/**
 * Counts up to `total` every `intervalMs` while `active`, then stops.
 * Drives the word / character / line typing of the step components.
 */
export function useTicker(active: boolean, intervalMs: number, total: number) {
  const [ticks, setTicks] = useState(0)
  const done = ticks >= total
  useEffect(() => {
    if (!active || done) return
    const id = setInterval(() => setTicks((t) => Math.min(t + 1, total)), intervalMs)
    return () => clearInterval(id)
  }, [active, done, intervalMs, total])
  return ticks
}
