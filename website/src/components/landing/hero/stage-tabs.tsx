"use client"

import { useLayoutEffect, useRef, useState } from "react"
import { LogoMark } from "@/components/site/logo"
import { cn } from "@/lib/utils"
import { STAGE_ORDER, STAGES, type Stage } from "./viz-data"

type StageTabsProps = {
  stage: Stage
  onSelect: (stage: Stage) => void
  className?: string
}

const tab =
  "relative z-0 inline-flex h-8 flex-1 items-center justify-center gap-1.5 whitespace-nowrap px-3.5 text-[13px] font-medium sm:flex-none"

/** The label with the mark on the iii tab, so the destination of the story is visible from any stage. */
function TabLabel({ stage }: { stage: Stage }) {
  return (
    <>
      {stage === "iii" && <LogoMark className="size-3" />}
      {STAGES[stage].tab}
    </>
  )
}

/**
 * Segmented control. The selected look is a second copy of the tab row
 * (inverted colors) clipped to the active tab; moving the clip gives a perfect
 * color handoff that per-tab color transitions can't match (Emil Kowalski's
 * clip-path tabs). The clip only starts transitioning after the first measure,
 * so there's no sweep on load.
 */
export function StageTabs({ stage, onSelect, className }: StageTabsProps) {
  const rowRef = useRef<HTMLDivElement>(null)
  const tabRefs = useRef(new Map<Stage, HTMLButtonElement>())
  const [clip, setClip] = useState<string | null>(null)
  const [ready, setReady] = useState(false)

  useLayoutEffect(() => {
    const row = rowRef.current
    const el = tabRefs.current.get(stage)
    if (!row || !el) return
    const measure = () => {
      const left = el.offsetLeft
      const right = row.offsetWidth - (el.offsetLeft + el.offsetWidth)
      setClip(`inset(0 ${right}px 0 ${left}px round 8px)`)
    }
    measure()
    const ro = new ResizeObserver(measure)
    ro.observe(row)
    return () => ro.disconnect()
  }, [stage])

  // Enable the transition one frame after the first clip is painted.
  useLayoutEffect(() => {
    if (clip && !ready) {
      const id = requestAnimationFrame(() => setReady(true))
      return () => cancelAnimationFrame(id)
    }
  }, [clip, ready])

  return (
    <div className={cn("relative rounded-[10px] bg-gray-3 p-0.5 shadow-[inset_0_0_0_1px_var(--gray-5)]", className)}>
      <div ref={rowRef} role="tablist" aria-label="Integration model" className="relative flex">
        {STAGE_ORDER.map((s) => (
          <button
            key={s}
            ref={(el) => {
              if (el) tabRefs.current.set(s, el)
            }}
            type="button"
            role="tab"
            aria-selected={s === stage}
            onClick={() => onSelect(s)}
            className={cn(
              tab,
              "cursor-pointer rounded-[8px] outline-none transition-colors duration-150 ease-[ease] hover:text-gray-12 focus-visible:ring-2 focus-visible:ring-gray-8",
              // The iii tab stays bright at rest: it's where the story is going.
              s === "iii" ? "text-gray-12" : "text-gray-10",
            )}
          >
            <TabLabel stage={s} />
          </button>
        ))}

        {/* The inverted copy, clipped to the active tab. */}
        <div
          aria-hidden="true"
          style={{ clipPath: clip ?? "inset(0 100% 0 0)" }}
          className={cn(
            "pointer-events-none absolute inset-0 flex rounded-[8px] bg-gray-12 shadow-[0_1px_2px_rgb(0_0_0/0.2)]",
            ready &&
              "transition-[clip-path] duration-300 ease-[cubic-bezier(0.77,0,0.175,1)] motion-reduce:transition-none",
          )}
        >
          {STAGE_ORDER.map((s) => (
            <span key={s} className={cn(tab, "text-gray-1")}>
              <TabLabel stage={s} />
            </span>
          ))}
        </div>
      </div>
    </div>
  )
}
