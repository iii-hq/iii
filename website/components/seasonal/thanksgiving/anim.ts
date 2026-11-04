'use client'

import { useEffect, useRef, useState } from 'react'

/*
 * A tiny timeline toolkit for the seasonal mark prototypes. Each mark is a pure drawing of a progress value `t`
 * (0 = the plain iii mark, 1 = fully dressed), so every stage of an idea is easy to tune and to freeze as a still.
 */

export const clamp01 = (x: number) => Math.min(1, Math.max(0, x))
/** Progress through the slice [a, b] of the timeline, clamped to 0..1. */
export const seg = (t: number, a: number, b: number) => clamp01((t - a) / (b - a))
export const lerp = (a: number, b: number, p: number) => a + (b - a) * p

export const easeInOut = (p: number) => (p < 0.5 ? 4 * p ** 3 : 1 - (-2 * p + 2) ** 3 / 2)
export const easeOut = (p: number) => 1 - (1 - p) ** 3
export const easeIn = (p: number) => p * p
/** Ease out with one small overshoot, for things that pop into place. */
export const backOut = (p: number, s = 1.7) => 1 + (s + 1) * (p - 1) ** 3 + s * (p - 1) ** 2

/** Blend two #rrggbb colours. */
export function mix(a: string, b: string, p: number): string {
  const x = Number.parseInt(a.slice(1), 16)
  const y = Number.parseInt(b.slice(1), 16)
  const ch = (shift: number) => Math.round(lerp((x >> shift) & 0xff, (y >> shift) & 0xff, clamp01(p)))
  return `rgb(${ch(16)} ${ch(8)} ${ch(0)})`
}

/** An SVG transform that scales around (x, y). */
export const scaleAt = (x: number, y: number, sx: number, sy = sx) =>
  `translate(${x} ${y}) scale(${Math.max(0, sx)} ${Math.max(0, sy)}) translate(${-x} ${-y})`

/**
 * A detail popping into place around (x, y): it grows from half size with a small overshoot while it fades in, so
 * nothing ever appears out of scale(0).
 */
export function pop(x: number, y: number, p: number, squashY = 1) {
  const s = p <= 0 ? 0.5 : lerp(0.5, 1, backOut(p))
  return { transform: scaleAt(x, y, s, s * squashY), opacity: clamp01(p * 2.5) }
}

/**
 * Runs `t` toward 1 while `on` and back toward 0 when it turns off, picking up from wherever it is. A `oneshot`
 * timeline instead plays 0 → 1 once per rising edge of `on` and snaps back to 0, ignoring `on` turning off midway.
 */
export function useTimeline(
  on: boolean,
  { duration, back = duration * 0.6, oneshot = false }: { duration: number; back?: number; oneshot?: boolean },
): number {
  const [t, setT] = useState(0)
  const current = useRef(0)
  const running = useRef(false)

  useEffect(() => {
    if (oneshot && (!on || running.current)) return
    const from = oneshot ? 0 : current.current
    const to = oneshot || on ? 1 : 0
    if (from === to) return

    const span = Math.abs(to - from) * (to > from ? duration : back) * 1000
    const start = performance.now()
    let frame = 0
    running.current = true
    const tick = (now: number) => {
      const p = Math.min(1, (now - start) / span)
      current.current = lerp(from, to, p)
      setT(current.current)
      if (p < 1) {
        frame = requestAnimationFrame(tick)
        return
      }
      running.current = false
      if (oneshot) {
        current.current = 0
        setT(0)
      }
    }
    frame = requestAnimationFrame(tick)
    if (oneshot) return
    return () => {
      cancelAnimationFrame(frame)
      running.current = false
    }
  }, [on, oneshot, duration, back])

  return t
}

/**
 * Seconds since `active` last turned on (times `speed`), ticking every frame; 0 while inactive. Drives idle loops
 * like blinking, so it only runs while a mark is held fully on.
 */
export function useClock(active: boolean, speed = 1): number {
  const [now, setNow] = useState(0)
  useEffect(() => {
    if (!active) {
      setNow(0)
      return
    }
    const start = performance.now()
    let frame = 0
    const tick = (time: number) => {
      setNow(((time - start) / 1000) * speed)
      frame = requestAnimationFrame(tick)
    }
    frame = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(frame)
  }, [active, speed])
  return now
}

/** 1, dipping to 0.15 for the last 0.18s of every `period`: a blink. */
export function blink(clock: number, period = 2.6): number {
  if (clock <= 0) return 1
  return clock % period > period - 0.18 ? 0.15 : 1
}
