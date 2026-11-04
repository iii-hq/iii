'use client'

import { useEffect, useRef, useState } from 'react'

import { CubeMark } from '../shared/cube-mark'
import { INK, type MarkProps } from '../shared/mark-svg'
import type { Idea } from '../shared/season-link'
import { FACES } from './faces'
import { DiyasMark, FireworksMark, RangoliMark, SparklerMark, ToranMark } from './marks'

/** The cube turn with a rotating cast: diya, akash kandil, laddoo, anar. Each play shows the next face. */
export function DiwaliCubeMark({ on, size, still }: MarkProps) {
  const [face, setFace] = useState(0)
  const wasOn = useRef(false)
  useEffect(() => {
    if (wasOn.current && !on) setFace((n) => (n + 1) % FACES.length)
    wasOn.current = on
  }, [on])
  return <CubeMark on={on || !!still} faces={FACES} face={face} size={size} ink={INK} />
}

/** The six ideas the site rotates through during Diwali, in order. */
export const IDEAS: readonly Idea[] = [
  { id: 'diyas', name: 'Diyas', Mark: DiyasMark, showMs: 2600, playMs: 0, settleMs: 900 },
  { id: 'rangoli', name: 'Rangoli', Mark: RangoliMark, showMs: 2600, playMs: 0, settleMs: 900 },
  { id: 'phuljhari', name: 'Phuljhari', Mark: SparklerMark, showMs: 2600, playMs: 0, settleMs: 600 },
  { id: 'fireworks', name: 'Fireworks', Mark: FireworksMark, showMs: 2600, playMs: 2700, settleMs: 200 },
  { id: 'toran', name: 'Marigold toran', Mark: ToranMark, showMs: 2800, playMs: 0, settleMs: 900 },
  { id: 'cube', name: 'Diwali cube', Mark: DiwaliCubeMark, showMs: 2600, playMs: 0, settleMs: 1100 },
]
