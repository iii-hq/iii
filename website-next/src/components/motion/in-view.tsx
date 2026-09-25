"use client"

import { type CSSProperties, type ReactNode, useEffect, useRef } from "react"

/**
 * Marks its box with `data-inview` the first time a third of it is on screen,
 * and leaves it there. Descendants key their one-shot draw-ins off the
 * attribute in CSS (see `.motif` in globals.css), so nothing re-runs on the
 * way back up and nothing animates before the reader can see it.
 */
export function InView({
  children,
  className,
  style,
}: {
  children: ReactNode
  className?: string
  style?: CSSProperties
}) {
  const ref = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const el = ref.current
    if (!el) return
    const io = new IntersectionObserver(
      ([entry]) => {
        if (entry.isIntersecting) {
          el.setAttribute("data-inview", "")
          io.disconnect()
        }
      },
      { threshold: 0.3 },
    )
    io.observe(el)
    return () => io.disconnect()
  }, [])
  return (
    <div ref={ref} className={className} style={style}>
      {children}
    </div>
  )
}
