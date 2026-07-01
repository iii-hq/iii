import { useCallback, useEffect, useRef, useState } from 'react'

export interface Stepper {
  step: number
  playing: boolean
  atEnd: boolean
  play: () => void
  pause: () => void
  toggle: () => void
  next: () => void
  prev: () => void
  reset: () => void
  goTo: (step: number) => void
}

/**
 * shared driver for the step-through diagrams: `step` walks 0..total-1,
 * auto-advancing every `intervalMs` while playing; pauses at the end.
 */
export function useStepper(total: number, intervalMs = 2400, autoPlay = false): Stepper {
  const [step, setStep] = useState(0)
  const [playing, setPlaying] = useState(autoPlay)
  const timer = useRef<ReturnType<typeof setInterval> | null>(null)

  const atEnd = step >= total - 1

  useEffect(() => {
    if (!playing) return
    timer.current = setInterval(() => {
      setStep((s) => {
        if (s >= total - 1) {
          setPlaying(false)
          return s
        }
        return s + 1
      })
    }, intervalMs)
    return () => {
      if (timer.current) clearInterval(timer.current)
    }
  }, [playing, intervalMs, total])

  const play = useCallback(() => {
    setStep((s) => (s >= total - 1 ? 0 : s))
    setPlaying(true)
  }, [total])
  const pause = useCallback(() => setPlaying(false), [])
  const toggle = useCallback(() => {
    setPlaying((p) => {
      if (!p) setStep((s) => (s >= total - 1 ? 0 : s))
      return !p
    })
  }, [total])
  const next = useCallback(() => {
    setPlaying(false)
    setStep((s) => Math.min(total - 1, s + 1))
  }, [total])
  const prev = useCallback(() => {
    setPlaying(false)
    setStep((s) => Math.max(0, s - 1))
  }, [])
  const reset = useCallback(() => {
    setPlaying(false)
    setStep(0)
  }, [])
  const goTo = useCallback(
    (s: number) => {
      setPlaying(false)
      setStep(Math.max(0, Math.min(total - 1, s)))
    },
    [total],
  )

  return { step, playing, atEnd, play, pause, toggle, next, prev, reset, goTo }
}
