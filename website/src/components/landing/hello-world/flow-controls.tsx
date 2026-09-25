"use client"

import { Swap } from "@/components/motion/swap"
import { ArrowRightIcon, PauseIcon, PlayIcon } from "@/components/site/iconly"
import { buttonVariants } from "@/components/ui/button-variants"
import { cn } from "@/lib/utils"
import { FLOW_STEPS } from "./flow-data"

export type FlowControlsProps = {
  index: number
  playing: boolean
  onPrev: () => void
  onNext: () => void
  onToggle: () => void
  onSeek: (i: number) => void
}

/**
 * Seven segments, one per step: done `bg-gray-8`, current `bg-gray-12`,
 * upcoming `bg-gray-5`. Each is a button with a 44px hit area, so the strip
 * doubles as a scrubber.
 */
export function Timeline({
  index,
  onSeek,
  className,
}: Pick<FlowControlsProps, "index" | "onSeek"> & { className?: string }) {
  return (
    <fieldset className={cn("flex min-w-0 gap-1.5", className)}>
      <legend className="sr-only">Steps</legend>
      {FLOW_STEPS.map((s, i) => (
        <button
          key={s.label}
          type="button"
          aria-label={`Step ${i + 1} of ${FLOW_STEPS.length}: ${s.label}`}
          aria-current={i === index ? "step" : undefined}
          onClick={() => onSeek(i)}
          className="group/seg flex h-11 flex-1 cursor-pointer items-center rounded-[8px] outline-none focus-visible:ring-2 focus-visible:ring-gray-8 focus-visible:ring-offset-2 focus-visible:ring-offset-gray-1"
        >
          <span
            aria-hidden="true"
            className={cn(
              "block h-[3px] w-full rounded-full transition-[background-color] duration-200 ease-[ease]",
              i === index ? "bg-gray-12" : i < index ? "bg-gray-8" : "bg-gray-5 group-hover/seg:bg-gray-7",
            )}
          />
        </button>
      ))}
    </fieldset>
  )
}

/**
 * Step counter and caption, plus one grouped cluster: Previous, Play/Pause,
 * Next. The cluster is a single bordered surface so the three read as one
 * control, with Play/Pause filled as the primary. When the row is too narrow
 * for both, the cluster drops under the caption and stretches edge to edge.
 */
export function Controls({
  index,
  playing,
  onPrev,
  onNext,
  onToggle,
  className,
}: Omit<FlowControlsProps, "onSeek"> & { className?: string }) {
  const step = FLOW_STEPS[index]
  const side = cn(
    buttonVariants({ variant: "ghost", size: "sm" }),
    "h-9 flex-1 gap-1.5 rounded-[7px] text-gray-11 hover:bg-gray-3 sm:flex-none",
  )
  return (
    <div className={cn("flex flex-wrap items-center gap-x-4 gap-y-3", className)}>
      <p className="flex min-w-0 flex-1 basis-[260px] items-baseline gap-3 text-[13px] text-gray-11">
        <span className="shrink-0 text-gray-10 tabular-nums">
          Step {index + 1} / {FLOW_STEPS.length}
        </span>
        <Swap id={index} className="min-w-0 truncate">
          {step.desc}
        </Swap>
      </p>
      <fieldset className="flex w-full items-center gap-1 rounded-[10px] bg-gray-2 p-1 shadow-[inset_0_0_0_1px_var(--gray-6)] sm:w-auto">
        <legend className="sr-only">Playback</legend>
        <button type="button" onClick={onPrev} className={cn(side, "pl-2.5")}>
          <ArrowRightIcon className="size-3.5 rotate-180" />
          Previous
        </button>
        <button
          type="button"
          onClick={onToggle}
          aria-pressed={playing}
          className={cn(buttonVariants({ size: "sm" }), "h-9 min-w-[88px] flex-1 gap-1.5 rounded-[7px] sm:flex-none")}
        >
          <Swap id={playing ? "pause" : "play"} className="gap-1.5">
            {playing ? <PauseIcon className="size-3" /> : <PlayIcon className="size-3" />}
            {playing ? "Pause" : "Play"}
          </Swap>
        </button>
        <button type="button" onClick={onNext} className={cn(side, "pr-2.5")}>
          Next
          <ArrowRightIcon className="size-3.5" />
        </button>
      </fieldset>
    </div>
  )
}
