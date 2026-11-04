'use client'

import { useReducedMotion } from 'motion/react'
import { useEffect, useState } from 'react'

import { IDEAS, type Idea } from '../thanksgiving/ideas'

/*
 * The six Thanksgiving marks the site rotates through, one card each. Every card plays its mark on a loop (staggered
 * so they take turns) and on hover. Not linked from anywhere and kept out of search.
 */

const REST_MS = 3400
const STAGGER_MS = 600

function Card({ idea, delay }: { idea: Idea; delay: number }) {
  const reduceMotion = useReducedMotion()
  const [hovered, setHovered] = useState(false)
  const [playing, setPlaying] = useState(false)

  useEffect(() => {
    if (reduceMotion) return
    let timer = 0
    const next = (wait: number) => {
      timer = window.setTimeout(() => {
        setPlaying(true)
        timer = window.setTimeout(() => {
          setPlaying(false)
          next(REST_MS)
        }, idea.showMs)
      }, wait)
    }
    next(delay)
    return () => window.clearTimeout(timer)
  }, [idea, delay, reduceMotion])

  const { Mark } = idea
  return (
    <div
      role="img"
      aria-label={idea.name}
      className="flex h-[240px] items-center justify-center rounded-xl border border-border bg-card"
      onPointerEnter={(e) => e.pointerType === 'mouse' && setHovered(true)}
      onPointerLeave={(e) => e.pointerType === 'mouse' && setHovered(false)}
    >
      {/* Readers who asked for less motion see each mark's finished pose. */}
      <Mark on={hovered || playing || !!reduceMotion} size={64} still={!!reduceMotion} />
    </div>
  )
}

export function SeasonalLab() {
  return (
    <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
      {IDEAS.map((idea, i) => (
        <Card key={idea.id} idea={idea} delay={800 + i * STAGGER_MS} />
      ))}
    </div>
  )
}
