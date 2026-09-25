"use client"

import { type TouchEvent, useEffect, useRef } from "react"
import { cn } from "@/lib/utils"
import type { CommentTokens, WorkerCode } from "../hello-world"
import { Controls, type FlowControlsProps, Timeline } from "./flow-controls"
import { FLOW_STEPS, lineStates, workerStatus } from "./flow-data"
import { WorkerCard } from "./worker-card"

const SWIPE_MIN = 48

/** Width of one slide (the track bleeds past the page margin, so its clientWidth is wider). */
function slideWidth(track: HTMLDivElement | null) {
  return (track?.firstElementChild as HTMLElement | null)?.offsetWidth ?? 0
}

type MobileFlowProps = FlowControlsProps & {
  code: WorkerCode
  comments: CommentTokens
  /** reduced motion: snap instead of smooth-scrolling */
  still: boolean
  onPause: () => void
}

/** <1024px: swipe the flow, one slide per step, each showing the worker it runs on. */
export function MobileFlow({
  code,
  comments,
  index,
  playing,
  still,
  onPrev,
  onNext,
  onToggle,
  onSeek,
  onPause,
}: MobileFlowProps) {
  const trackRef = useRef<HTMLDivElement>(null)
  // true while *we* scroll the track, so the scroll handler doesn't seek
  const syncing = useRef(false)
  const scrollTimer = useRef<number | undefined>(undefined)
  const swipe = useRef<{ x: number; y: number; onTrack: boolean } | null>(null)

  // Step changed: bring its slide into view.
  useEffect(() => {
    const track = trackRef.current
    const width = slideWidth(track)
    if (!track || !width) return
    const target = index * width
    if (Math.abs(track.scrollLeft - target) <= 4) return
    syncing.current = true
    track.scrollTo({ left: target, behavior: still ? "auto" : "smooth" })
    const done = window.setTimeout(() => {
      syncing.current = false
    }, 500)
    return () => {
      window.clearTimeout(done)
      syncing.current = false
    }
  }, [index, still])

  useEffect(() => () => window.clearTimeout(scrollTimer.current), [])

  // User swiped the carousel natively: seek to wherever it snapped.
  const onTrackScroll = () => {
    if (syncing.current) return
    window.clearTimeout(scrollTimer.current)
    scrollTimer.current = window.setTimeout(() => {
      const track = trackRef.current
      const width = slideWidth(track)
      if (!track || !width) return
      const i = Math.round(track.scrollLeft / width)
      if (i !== index && i >= 0 && i < FLOW_STEPS.length) onSeek(i)
    }, 120)
  }

  // Swipes that start off the scrollable track (e.g. on the timeline) still page.
  const onBodyTouchStart = (e: TouchEvent) => {
    if (playing) onPause()
    const t = e.touches[0]
    swipe.current =
      e.touches.length === 1 && t
        ? { x: t.clientX, y: t.clientY, onTrack: !!trackRef.current?.contains(e.target as Node) }
        : null
  }
  const onBodyTouchEnd = (e: TouchEvent) => {
    const start = swipe.current
    const t = e.changedTouches[0]
    swipe.current = null
    if (!start || !t || start.onTrack) return
    const dx = t.clientX - start.x
    const dy = t.clientY - start.y
    if (Math.abs(dx) < SWIPE_MIN || Math.abs(dx) < Math.abs(dy)) return
    onSeek(dx < 0 ? index + 1 : index - 1)
  }

  return (
    <div className="lg:hidden" onTouchStart={onBodyTouchStart} onTouchEnd={onBodyTouchEnd}>
      <Timeline index={index} onSeek={onSeek} />

      {/* Slides bleed to the viewport edge so the next card can peek in; the cards keep the page margin. */}
      <div
        ref={trackRef}
        onScroll={onTrackScroll}
        className="-mx-5 mt-1 flex snap-x snap-mandatory scroll-pl-5 overflow-x-auto overflow-y-hidden px-5 pb-2 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
      >
        {FLOW_STEPS.map((s, i) => (
          <div
            key={s.label}
            aria-hidden={i !== index}
            className={cn("w-full min-w-0 flex-[0_0_100%] snap-start snap-always pr-3")}
          >
            <WorkerCard
              worker={s.active}
              variant="mobile"
              lines={code[s.active]}
              status={workerStatus(s, s.active)}
              states={lineStates(s, s.active)}
              revealed={s.comments}
              comments={comments}
              className="h-full"
            />
          </div>
        ))}
      </div>

      <Controls index={index} playing={playing} onPrev={onPrev} onNext={onNext} onToggle={onToggle} className="mt-4" />
    </div>
  )
}
