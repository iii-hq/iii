'use client'

import { useInView, useReducedMotion } from 'motion/react'
import { type ComponentType, useEffect, useRef, useState } from 'react'

import type { MarkProps } from './mark-svg'

/** One seasonal mark in a season's rotation. */
export type Idea = {
  id: string
  name: string
  Mark: ComponentType<MarkProps>
  /** How long an autoplay holds the mark on. */
  showMs: number
  /** How long one play runs once started, even if `on` drops; only play-once ideas need it. */
  playMs: number
  /** After `on` drops, how long until the mark is back at rest and the next idea can take over. */
  settleMs: number
}

/** Autoplay: the first play this long after the mark comes on screen... */
const FIRST_MS = 1750
/**
 * ...then a pause this long after each play before the next one. With a ~2.6s play that is a new idea about every
 * 6s, 30% quicker than the Halloween mark's 8.6s.
 */
const REST_MS = 3400

/** What the header and footer pass to a season's link. */
export type SeasonLinkProps = {
  href: string
  label: string
  /** The mark's size in px. */
  size: number
  /** Play on its own every few seconds while on screen (default on). */
  autoplay?: boolean
  className?: string
}

/**
 * A link wearing a season's iii mark. It plays on hover or keyboard focus and, with `autoplay`, on its own every few
 * seconds while it is on screen. Each play shows the next of the season's `ideas`, starting from a random one so a
 * returning reader meets something new.
 */
export function SeasonLink({
  ideas,
  href,
  label,
  size,
  autoplay = true,
  className,
}: SeasonLinkProps & { ideas: readonly Idea[] }) {
  const ref = useRef<HTMLAnchorElement>(null)
  const inView = useInView(ref)
  const reduceMotion = useReducedMotion()
  const [hovered, setHovered] = useState(false)
  const [visiting, setVisiting] = useState(false)
  const [index, setIndex] = useState(0)
  const on = hovered || visiting

  const idea = ideas[index]
  const ideaRef = useRef(idea)
  ideaRef.current = idea

  // Picked after mount so the server and client markup agree.
  useEffect(() => setIndex(Math.floor(Math.random() * ideas.length)), [ideas.length])

  // Once a play ends and the mark is back at rest, queue the next idea. A play-once idea (a trot, a firework) keeps going after
  // the pointer leaves, so wait for it to finish too; hovering again before then replays the same one, so the mark
  // never swaps mid-animation.
  const played = useRef(false)
  const busyUntil = useRef(0)
  useEffect(() => {
    if (on) {
      played.current = true
      busyUntil.current = performance.now() + idea.playMs
      return
    }
    if (!played.current) return
    const wait = Math.max(idea.settleMs, busyUntil.current - performance.now())
    const timer = window.setTimeout(() => {
      played.current = false
      setIndex((n) => (n + 1) % ideas.length)
    }, wait)
    return () => window.clearTimeout(timer)
  }, [on, idea, ideas.length])

  // Autoplay: only while the mark is on screen and the tab is visible, and never for readers who asked for less motion.
  useEffect(() => {
    if (!autoplay || reduceMotion || !inView) return
    let timer = 0
    const next = (wait: number) => {
      timer = window.setTimeout(() => {
        if (document.hidden) return next(REST_MS)
        setVisiting(true)
        timer = window.setTimeout(() => {
          setVisiting(false)
          next(REST_MS)
        }, ideaRef.current.showMs)
      }, wait)
    }
    next(FIRST_MS)
    return () => {
      window.clearTimeout(timer)
      setVisiting(false)
    }
  }, [autoplay, reduceMotion, inView])

  const { Mark } = idea
  return (
    <a
      ref={ref}
      href={href}
      aria-label={label}
      className={className}
      data-idea={idea.id}
      data-playing={on || undefined}
      // Touch fires a hover on tap, which would start a play right as the page navigates away.
      onPointerEnter={(e) => e.pointerType === 'mouse' && setHovered(true)}
      onPointerLeave={(e) => e.pointerType === 'mouse' && setHovered(false)}
      onFocus={() => setHovered(true)}
      onBlur={() => setHovered(false)}
    >
      {/* Readers who asked for less motion get the finished pose without the animation. */}
      <Mark on={on} size={size} still={!!reduceMotion && on} />
    </a>
  )
}
