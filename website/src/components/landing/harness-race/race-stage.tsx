"use client"

import { useInView, useReducedMotion } from "motion/react"
import { type RefObject, useRef, useState, useSyncExternalStore } from "react"
import { buttonVariants } from "@/components/ui/button-variants"
import { usePageVisible } from "@/hooks/use-page-visible"
import { cn } from "@/lib/utils"
import { AgentWindow } from "./agent-window"
import { IiiWindow } from "./iii-window"
import { DUR, SCENES } from "./script"
import { raceFrame, type TokenMap, totalDuration, useLoopClock } from "./timeline"
import { Verdict } from "./verdict"

const TOTAL = totalDuration(SCENES)
/** Reduced motion holds the loop's last frame: both transcripts complete, verdicts shown. */
const HOLD_TIME = TOTAL - 0.001

// True only after hydration. The server can't know the reader's motion
// preference, so the first client render must match the server's frame.
const noop = () => () => {
  /* nothing to unsubscribe: hydration happens once */
}
const useHydrated = () =>
  useSyncExternalStore(
    noop,
    () => true,
    () => false,
  )

/**
 * The race's playhead. Plays while `box` is on screen and the tab is visible;
 * reduced motion holds the final frame until the reader presses play.
 */
function useRacePlayback(box: RefObject<HTMLDivElement | null>) {
  const hydrated = useHydrated()
  const prefersReduced = useReducedMotion() ?? false
  const reduced = hydrated && prefersReduced
  const [wants, setWants] = useState<boolean | null>(null)
  const tabVisible = usePageVisible()

  // Mounted on arrival, so the reader meets step 0 rather than whatever step
  // the clock reached while the section was still below the fold. After that
  // it loops, pausing while off screen.
  const entered = useInView(box, { amount: 0.3, once: true })
  const visible = useInView(box)
  const autoplay = wants ?? !reduced
  const playing = autoplay && entered && visible && tabVisible
  const time = useLoopClock(TOTAL, playing)
  const held = reduced && wants === null
  const t = held ? HOLD_TIME : time
  const ready = entered || held
  const toggle = () => setWants(!autoplay)
  return { t, ready, autoplay, toggle }
}

/**
 * The race: both windows on one clock, side by side from 720px and stacked
 * below. Plays while on screen and the tab is visible; reduced motion holds
 * the final frame until the reader presses play.
 */
export function RaceStage({ code, className }: { code: TokenMap; className?: string }) {
  const box = useRef<HTMLDivElement>(null)
  const { t, ready, autoplay, toggle } = useRacePlayback(box)
  const { tradStep, tradProgress, iiiStep, iiiProgress, elapsed, phase, trad, iii } = raceFrame(t, ready)

  const control = (
    <button
      type="button"
      onClick={toggle}
      aria-label={autoplay ? "Pause the race" : "Play the race"}
      className={cn(
        buttonVariants({ variant: "ghost", size: "sm" }),
        // 44px touch target on a 32px button
        "relative -mr-3 after:absolute after:-inset-1.5 after:content-['']",
      )}
    >
      {autoplay ? "Pause" : "Play"}
    </button>
  )

  return (
    <div
      ref={box}
      className={cn("grid gap-5 min-[720px]:grid-cols-2 min-[720px]:gap-x-6 min-[720px]:gap-y-4", className)}
    >
      <AgentWindow
        step={tradStep}
        progress={tradProgress}
        dur={DUR[Math.min(tradStep, DUR.length - 1)]}
        code={code}
        elapsed={elapsed}
      />
      <IiiWindow
        step={iiiStep}
        progress={iiiProgress}
        dur={DUR[Math.min(iiiStep, DUR.length - 1)]}
        code={code}
        versus={trad}
        elapsed={elapsed}
        aside={control}
      />
      <Verdict trad={trad} iii={iii} phase={phase} />
    </div>
  )
}
