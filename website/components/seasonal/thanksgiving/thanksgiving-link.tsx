'use client'

import { useInView, useReducedMotion } from 'motion/react'
import { useEffect, useRef, useState } from 'react'

import { IDEAS } from './ideas'

/** The header drops in once per browsing session, this long after the page loads. */
const DROP_IN_MS = 2500
const DROP_IN_KEY = 'iii-thanksgiving-drop-in'

/**
 * A link wearing the iii mark for Thanksgiving week. It plays on hover or keyboard focus and, with `dropIn`, once on
 * its own shortly after the first page load of a visit. Each play shows the next of the six ideas, starting from a
 * random one so a returning reader meets something new.
 */
export default function ThanksgivingLink({
  href,
  label,
  size,
  dropIn = false,
  className,
}: {
  href: string
  label: string
  /** The mark's size in px. */
  size: number
  dropIn?: boolean
  className?: string
}) {
  const ref = useRef<HTMLAnchorElement>(null)
  const inView = useInView(ref)
  const reduceMotion = useReducedMotion()
  const [hovered, setHovered] = useState(false)
  const [visiting, setVisiting] = useState(false)
  const [index, setIndex] = useState(0)
  const on = hovered || visiting

  const idea = IDEAS[index]
  const ideaRef = useRef(idea)
  ideaRef.current = idea

  // Picked after mount so the server and client markup agree.
  useEffect(() => setIndex(Math.floor(Math.random() * IDEAS.length)), [])

  // Once a play ends and the mark has settled back to rest, queue the next idea. Hovering again before then replays
  // the same one, so a face never swaps mid-animation.
  const played = useRef(false)
  useEffect(() => {
    if (on) {
      played.current = true
      return
    }
    if (!played.current) return
    const timer = window.setTimeout(() => {
      played.current = false
      setIndex((n) => (n + 1) % IDEAS.length)
    }, idea.settleMs)
    return () => window.clearTimeout(timer)
  }, [on, idea])

  // The drop-in: once per session, only while the mark is on screen and the tab is visible, and never for readers who
  // asked for less motion.
  useEffect(() => {
    if (!dropIn || reduceMotion || !inView) return
    try {
      if (window.sessionStorage.getItem(DROP_IN_KEY)) return
    } catch {
      // Storage blocked: still drop in, just without remembering it.
    }
    let hold = 0
    const start = window.setTimeout(() => {
      if (document.hidden) return
      try {
        window.sessionStorage.setItem(DROP_IN_KEY, '1')
      } catch {
        // Storage blocked: the drop-in may repeat on the next page, which is harmless.
      }
      setVisiting(true)
      hold = window.setTimeout(() => setVisiting(false), ideaRef.current.showMs)
    }, DROP_IN_MS)
    return () => {
      window.clearTimeout(start)
      window.clearTimeout(hold)
      setVisiting(false)
    }
  }, [dropIn, reduceMotion, inView])

  const { Mark } = idea
  return (
    <a
      ref={ref}
      href={href}
      aria-label={label}
      className={className}
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
