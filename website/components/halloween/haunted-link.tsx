'use client'

import { useInView, useReducedMotion } from 'motion/react'
import { useEffect, useRef, useState } from 'react'

import { HauntedMark } from './haunted-logo'
import { MONSTERS } from './monsters'

/** A link wearing the iii mark for Halloween: it haunts on hover, and a monster also drops by on its own. */
const FIRST_MS = 2500
/** Long enough for the turn, the extras popping out, and one blink. */
const SHOW_MS = 2600
const REST_MS = 6000

/** oklch(0.97 0 0), the site's dark-theme foreground, as the hex the colour animation can interpolate from. */
const FOREGROUND = '#f5f5f5'

export function HauntedLink({
  href,
  label,
  size,
  className,
}: {
  href: string
  label: string
  /** CSS length. Multiples of 16px keep the pixel art on whole pixels. */
  size: string
  className?: string
}) {
  const ref = useRef<HTMLAnchorElement>(null)
  const inView = useInView(ref)
  const [hovered, setHovered] = useState(false)
  const [visiting, setVisiting] = useState(false)
  const [monster, setMonster] = useState(0)
  const reduceMotion = useReducedMotion()

  const on = hovered || visiting

  // Each visit ends with the next monster queued, starting from a random one so a returning reader meets someone
  // new. Picked after mount so the server and client markup agree.
  const wasOn = useRef(false)
  useEffect(() => {
    if (wasOn.current && !on) setMonster((n) => (n + 1) % MONSTERS.length)
    wasOn.current = on
  }, [on])
  useEffect(() => setMonster(Math.floor(Math.random() * MONSTERS.length)), [])

  // The drop-ins: only while the mark is on screen and the tab is visible, and never for readers who asked for less
  // motion.
  useEffect(() => {
    if (reduceMotion || !inView) return
    let timer: number
    const next = (wait: number) => {
      timer = window.setTimeout(() => {
        if (document.hidden) return next(REST_MS)
        setVisiting(true)
        timer = window.setTimeout(() => {
          setVisiting(false)
          next(REST_MS)
        }, SHOW_MS)
      }, wait)
    }
    next(FIRST_MS)
    return () => {
      window.clearTimeout(timer)
      setVisiting(false)
    }
  }, [reduceMotion, inView])

  return (
    <a
      ref={ref}
      href={href}
      aria-label={label}
      className={className}
      onPointerEnter={(e) => e.pointerType === 'mouse' && setHovered(true)}
      onPointerLeave={(e) => e.pointerType === 'mouse' && setHovered(false)}
      onFocus={() => setHovered(true)}
      onBlur={() => setHovered(false)}
    >
      <HauntedMark on={on} monster={monster} size={size} ink={FOREGROUND} />
    </a>
  )
}
