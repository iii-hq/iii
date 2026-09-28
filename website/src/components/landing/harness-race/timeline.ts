// The race's timeline engine: a looping scene clock plus the typing model.
// Ported from the vendored omelette engine (animations-v2.jsx), keeping only
// what the race uses: scene sequencing with hard cuts, the time-stretch of a
// scene's clock onto its authored length, and the loop. Everything visible is
// a pure function of the clock, so any frame can be rendered from a time.

import { startTransition, useEffect, useState } from "react"
import type { CodeLine } from "@/lib/shiki"
import { III_SCENES, iiiTokens, type Line, SCENES, type Scene, TYPED, tradTokens } from "./script"

export const clamp = (v: number, min: number, max: number) => Math.max(min, Math.min(max, v))

export const easeOutCubic = (t: number) => {
  const u = t - 1
  return u * u * u + 1
}

export const totalDuration = (scenes: Scene[]) => Math.round(scenes.reduce((sum, s) => sum + s.dur, 0) * 1000) / 1000

export type SceneFrame = {
  scene: Scene
  index: number
  /** 0..1 through the scene */
  progress: number
}

/** The scene under the playhead. `t === total` belongs to the last scene. */
export function sceneAt(scenes: Scene[], t: number): SceneFrame {
  let start = 0
  let index = scenes.length - 1
  for (let i = 0; i < scenes.length; i++) {
    if (t < start + scenes[i].dur) {
      index = i
      break
    }
    start += scenes[i].dur
  }
  if (index === scenes.length - 1) start = scenes.slice(0, -1).reduce((sum, s) => sum + s.dur, 0)
  const scene = scenes[index]
  const wall = clamp(t - start, 0, scene.dur)
  // Time-stretch: the scene clock runs 0..nat over `dur` wall seconds, so
  // progress is wall/dur whichever length the scene was authored at.
  return { scene, index, progress: scene.dur > 0 ? wall / scene.dur : 0 }
}

/**
 * Seconds into a looping timeline of `total` seconds, advanced by rAF while
 * `playing`. Pausing holds the playhead; resuming continues from it.
 */
export function useLoopClock(total: number, playing: boolean, initial = 0) {
  const [time, setTime] = useState(initial)
  useEffect(() => {
    if (!playing) return
    let raf = 0
    let last: number | null = null
    const tick = (ts: number) => {
      const dt = last === null ? 0 : (ts - last) / 1000
      last = ts
      startTransition(() => setTime((t) => (t + dt) % total))
      raf = requestAnimationFrame(tick)
    }
    raf = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(raf)
  }, [playing, total])
  return time
}

export type RaceFrame = {
  tradStep: number
  tradProgress: number
  iiiStep: number
  iiiProgress: number
  /** seconds shown on both timers */
  elapsed: number
  /** 0 before either side starts, 2 once iii has deployed, 1 in between */
  phase: 0 | 1 | 2
  /** left side's context, k tokens */
  trad: number
  /** right side's context, k tokens */
  iii: number
}

/**
 * Both windows at playhead `t`. One playhead, two clocks: the traditional
 * agent on the full scene list, iii on its compressed one, so iii finishes
 * first and then holds. Before `ready` every value is the opening frame.
 */
export function raceFrame(t: number, ready: boolean): RaceFrame {
  const tradFrame = sceneAt(SCENES, t)
  const iiiFrame = sceneAt(III_SCENES, t)
  const tradStep = ready ? tradFrame.scene.step : 0
  const tradProgress = ready ? tradFrame.progress : 0
  const iiiStep = ready ? iiiFrame.scene.step : 0
  const iiiProgress = ready ? iiiFrame.progress : 0
  const elapsed = ready ? t : 0
  const last = SCENES.length - 1
  const phase = iiiStep === 0 && tradStep === 0 ? 0 : iiiStep >= last ? 2 : 1
  return {
    tradStep,
    tradProgress,
    iiiStep,
    iiiProgress,
    elapsed,
    phase,
    trad: tradTokens(tradStep, tradProgress),
    iii: iiiTokens(iiiStep, iiiProgress),
  }
}

export type TypedRow = {
  line: Line
  /** characters shown; -1 once the line is complete */
  chars: number
  caret: boolean
  opacity: number
  /** 1 → 0 glow right after a line with a flash/answer/highlight finishes */
  flash: number
}

const lineCost = (line: Line) => (TYPED.has(line.k) ? String(line.t).length : 10)

/**
 * Rows visible at `progress` through `step`: every earlier step in full, then
 * the current step typing at `cps` chars/s after its think-pause (`delayFrac` of `dur`).
 */
export function typeSteps(
  data: Line[][],
  step: number,
  progress: number,
  dur: number,
  delayFrac = 0,
  cps = 90,
): TypedRow[] {
  const out: TypedRow[] = []
  const elapsed = progress * dur - delayFrac * dur
  for (let g = 0; g <= step && g < data.length; g++) {
    const group = data[g]
    if (g !== step) {
      for (const line of group) out.push({ line, chars: -1, caret: false, opacity: 1, flash: 0 })
      continue
    }
    if (elapsed <= 0) continue
    const costs = group.map(lineCost)
    const total = costs.reduce((a, b) => a + b, 0) || 1
    const head = Math.min(elapsed * cps, total)
    let acc = 0
    group.forEach((line, i) => {
      const start = acc
      const end = acc + costs[i]
      acc = end
      if (head <= start) return
      let chars = -1
      let caret = false
      let opacity = 1
      if (head < end) {
        if (TYPED.has(line.k)) {
          chars = Math.floor(head - start)
          caret = true
        } else {
          opacity = clamp((head - start) / costs[i], 0, 1)
        }
      }
      const flash =
        (line.flash || line.ans || line.hl) && head >= end ? clamp(1 - (elapsed - end / cps) / 1.2, 0, 1) : 0
      out.push({ line, chars, caret, opacity, flash })
    })
  }
  return out
}

/** The think-pause before a step starts typing. */
export function isThinking(step: number, progress: number, dur: number, delayFrac = 0) {
  return step > 0 && step < 7 && progress * dur < delayFrac * dur
}

/** The row's text up to `chars` (all of it once complete). */
export const sliceLine = (line: Line, chars: number) =>
  typeof line.t === "string" ? (chars < 0 ? line.t : line.t.slice(0, Math.max(0, chars))) : ""

/** Shiki-tokenized lines, keyed by the line's text; built on the server, rendered by the windows. */
export type TokenMap = Record<string, CodeLine>

/** The first `chars` characters of a tokenized line (all of it when `chars < 0`). */
export function sliceTokens(tokens: CodeLine, chars: number): CodeLine {
  if (chars < 0) return tokens
  const out: CodeLine = []
  let left = chars
  for (const t of tokens) {
    if (left <= 0) break
    out.push(left >= t.text.length ? t : { ...t, text: t.text.slice(0, left) })
    left -= t.text.length
  }
  return out
}
