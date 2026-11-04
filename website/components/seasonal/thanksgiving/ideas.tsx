'use client'

import { useEffect, useRef, useState } from 'react'

import { CubeMark } from '../shared/cube-mark'
import { INK, type MarkProps } from '../shared/mark-svg'
import type { Idea } from '../shared/season-link'
import { FACES } from './faces'
import { ChorusMark, GobbleMark, StrutMark, TrotMark } from './marks'

/** The Halloween cube turn showing the pixel turkey. */
export function TurkeyCubeMark({ on, size, still }: MarkProps) {
  return <CubeMark on={on || !!still} faces={FACES} face={0} size={size} ink={INK} />
}

/** The cube turn with a rotating cast: turkey, pumpkin pie, maple leaf, corn. Each play shows the next face. */
export function LineupCubeMark({ on, size, still }: MarkProps) {
  const [face, setFace] = useState(0)
  const wasOn = useRef(false)
  useEffect(() => {
    if (wasOn.current && !on) setFace((n) => (n + 1) % FACES.length)
    wasOn.current = on
  }, [on])
  return <CubeMark on={on || !!still} faces={FACES} face={face} size={size} ink={INK} />
}

/** The six ideas the site rotates through during Thanksgiving week, in order. */
export const IDEAS: readonly Idea[] = [
  { id: 'strut', name: 'Strut', Mark: StrutMark, showMs: 2600, playMs: 0, settleMs: 900 },
  { id: 'gobble', name: 'Gobble', Mark: GobbleMark, showMs: 2600, playMs: 0, settleMs: 700 },
  {
    id: 'turkey-cube',
    name: 'Turkey cube',
    Mark: TurkeyCubeMark,
    showMs: 2600,
    playMs: 0,
    settleMs: 1100,
  },
  {
    id: 'lineup',
    name: 'Harvest lineup',
    Mark: LineupCubeMark,
    showMs: 2600,
    playMs: 0,
    settleMs: 1100,
  },
  { id: 'trot', name: 'Turkey trot', Mark: TrotMark, showMs: 2800, playMs: 2900, settleMs: 200 },
  { id: 'chorus', name: 'Gobble chorus', Mark: ChorusMark, showMs: 3000, playMs: 0, settleMs: 900 },
]
