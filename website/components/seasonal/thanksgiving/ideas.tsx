'use client'

import { type ComponentType, useEffect, useRef, useState } from 'react'

import { CubeMark } from './cube-mark'
import { FACES } from './faces'
import { ChorusMark, GobbleMark, INK, type MarkProps, StrutMark, TrotMark } from './marks'

/** The Halloween cube turn showing the pixel turkey. */
export function TurkeyCubeMark({ on, size, still }: MarkProps) {
  return <CubeMark on={on || !!still} face={0} size={size} ink={INK} />
}

/** The cube turn with a rotating cast: turkey, pumpkin pie, maple leaf, corn. Each play shows the next face. */
export function LineupCubeMark({ on, size, still }: MarkProps) {
  const [face, setFace] = useState(0)
  const wasOn = useRef(false)
  useEffect(() => {
    if (wasOn.current && !on) setFace((n) => (n + 1) % FACES.length)
    wasOn.current = on
  }, [on])
  return <CubeMark on={on || !!still} face={face} size={size} ink={INK} />
}

export type Idea = {
  id: string
  name: string
  Mark: ComponentType<MarkProps>
  /** Plays once per trigger instead of holding while `on`. */
  oneshot?: boolean
  /** Driven by springs rather than a timeline, so it cannot be slowed down or scrubbed. */
  springs?: boolean
  /** How long a drop-in holds the mark on. */
  showMs: number
  /** After `on` drops, how long until the mark is back at rest and the next idea can take over. */
  settleMs: number
}

/** The six ideas the site rotates through during Thanksgiving week, in order. */
export const IDEAS: readonly Idea[] = [
  { id: 'strut', name: 'Strut', Mark: StrutMark, showMs: 2600, settleMs: 900 },
  { id: 'gobble', name: 'Gobble', Mark: GobbleMark, showMs: 2600, settleMs: 700 },
  { id: 'turkey-cube', name: 'Turkey cube', Mark: TurkeyCubeMark, springs: true, showMs: 2600, settleMs: 1100 },
  { id: 'lineup', name: 'Harvest lineup', Mark: LineupCubeMark, springs: true, showMs: 2600, settleMs: 1100 },
  { id: 'trot', name: 'Turkey trot', Mark: TrotMark, oneshot: true, showMs: 400, settleMs: 3000 },
  { id: 'chorus', name: 'Gobble chorus', Mark: ChorusMark, showMs: 3000, settleMs: 900 },
]
