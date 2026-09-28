"use client"

import { MotionConfig, useInView } from "motion/react"
import { useRef } from "react"
import { usePageVisible } from "@/hooks/use-page-visible"
import { track } from "@/lib/analytics"
import { cn } from "@/lib/utils"
import type { CommentTokens, WorkerCode } from "../hello-world"
import { DesktopFlow } from "./desktop-flow"
import { dwellFor, FLOW_STEPS } from "./flow-data"
import { MobileFlow } from "./mobile-flow"
import { usePrefersReducedMotion, useStepper } from "./use-stepper"

type ControlGroup = "hello_flow" | "hello_flow_mobile"

type HelloFlowProps = {
  code: WorkerCode
  comments: CommentTokens
  className?: string
}

/**
 * The execution-flow walkthrough. One stepper drives both layouts (desktop
 * cards + strip, mobile swipe carousel). Autoplay pauses while off screen or
 * in a background tab, and starts paused for reduced-motion users.
 */
export function HelloFlow({ code, comments, className }: HelloFlowProps) {
  const ref = useRef<HTMLDivElement>(null)
  const inView = useInView(ref)
  const visible = usePageVisible()
  const reduced = usePrefersReducedMotion()
  const { index, playing, goTo, next, prev, pause, toggle } = useStepper({
    count: FLOW_STEPS.length,
    dwell: dwellFor,
    active: inView && visible,
    autoplay: !reduced,
  })

  const controls = (group: ControlGroup) => ({
    onPrev: () => {
      track("flow_control", { control_group: group, action: "prev" })
      prev()
    },
    onNext: () => {
      track("flow_control", { control_group: group, action: "next" })
      next()
    },
    onToggle: () => {
      track("flow_control", { control_group: group, action: playing ? "pause" : "play" })
      toggle()
    },
    onSeek: goTo,
  })

  return (
    <MotionConfig reducedMotion="user">
      <div ref={ref} className={cn(className)}>
        <DesktopFlow code={code} comments={comments} index={index} playing={playing} {...controls("hello_flow")} />
        <MobileFlow
          code={code}
          comments={comments}
          index={index}
          playing={playing}
          still={reduced}
          onPause={pause}
          {...controls("hello_flow_mobile")}
        />
      </div>
    </MotionConfig>
  )
}
