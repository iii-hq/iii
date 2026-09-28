"use client"

import { m, useReducedMotion } from "motion/react"
import { useEffect, useId, useState } from "react"
import { cn } from "@/lib/utils"

export type TocItem = { id: string; text: string; depth?: 2 | 3 }

export type TocGroup = {
  /** small label over the group; omit on the first group */
  label?: string
  items: TocItem[]
}

/** Headings at or above this line (the fixed header plus a little room) count as passed. */
const LINE = 96

/**
 * A contents list beside a long document. The last heading to pass the top of
 * the viewport is the current one: its label goes to ink and a short marker
 * on the rail slides to it (a shared-layout spring, so it travels rather than
 * jumps). Hover lifts a link's colour; nothing else moves.
 */
export function TocRail({
  title = "Contents",
  groups,
  className,
}: {
  title?: string
  groups: TocGroup[]
  className?: string
}) {
  const reduce = useReducedMotion()
  const layoutId = useId()
  const key = groups.flatMap((g) => g.items.map((i) => i.id)).join("\n")
  const [active, setActive] = useState<string | null>(null)

  useEffect(() => {
    const headings = key
      .split("\n")
      .map((id) => document.getElementById(id))
      .filter((el): el is HTMLElement => el !== null)
    if (!headings.length) return
    let frame = 0
    const pick = () => {
      frame = 0
      let current = headings[0].id
      for (const h of headings) {
        if (h.getBoundingClientRect().top <= LINE) current = h.id
        else break
      }
      setActive(current)
    }
    const onScroll = () => {
      if (!frame) frame = requestAnimationFrame(pick)
    }
    pick()
    window.addEventListener("scroll", onScroll, { passive: true })
    window.addEventListener("resize", onScroll)
    return () => {
      if (frame) cancelAnimationFrame(frame)
      window.removeEventListener("scroll", onScroll)
      window.removeEventListener("resize", onScroll)
    }
  }, [key])

  if (!key) return null

  return (
    <nav aria-label={title} className={cn("text-[13px] leading-[1.5]", className)}>
      <p className="font-medium tracking-[0.02em] text-gray-10">{title}</p>
      {groups.map((group, gi) => (
        <div key={group.label ?? gi} className={gi === 0 ? "mt-3" : "mt-5"}>
          {group.label && <p className="mb-1.5 text-[12px] text-gray-9">{group.label}</p>}
          {/* The rail: a hairline the marker rides on. */}
          <ul className="relative flex flex-col border-l border-gray-5">
            {group.items.map((item) => {
              const isActive = item.id === active
              return (
                <li key={item.id} className="relative">
                  {isActive && (
                    <m.span
                      layoutId={layoutId}
                      aria-hidden="true"
                      className="absolute top-1.5 bottom-1.5 -left-px w-px bg-gray-12"
                      transition={reduce ? { duration: 0 } : { type: "spring", duration: 0.3, bounce: 0 }}
                    />
                  )}
                  <a
                    href={`#${item.id}`}
                    aria-current={isActive ? "location" : undefined}
                    className={cn(
                      "block rounded-r-[6px] py-1.5 pr-2 text-pretty outline-none",
                      "transition-[color,background-color] duration-150 ease-out hover:bg-gray-3 hover:text-gray-12",
                      "focus-visible:ring-2 focus-visible:ring-gray-8 focus-visible:ring-offset-2 focus-visible:ring-offset-gray-1",
                      item.depth === 3 ? "pl-7" : "pl-4",
                      isActive ? "text-gray-12" : "text-gray-10",
                    )}
                  >
                    {item.text}
                  </a>
                </li>
              )
            })}
          </ul>
        </div>
      ))}
    </nav>
  )
}
