import type { ReactNode } from "react"
import { Swap } from "@/components/motion/swap"
import { cn } from "@/lib/utils"
import { CostMeter } from "./cost-meter"
import { MAX_TOK } from "./script"

type RaceWindowProps = {
  /** title bar label, e.g. "Traditional agent" */
  label: string
  /** leading slot before the label (the iii mark) */
  icon?: ReactNode
  /** the icon carries the identity; keep the label for assistive tech only */
  labelHidden?: boolean
  /** k tokens this side has used; drives the spend meter along the bottom */
  tokens: number
  /** the side being sold: a ring on the window and an ink meter */
  emphasis?: boolean
  /** the other side's k tokens, for the "N× fewer" pill in the meter */
  versus?: number
  /** wall-clock seconds shown beside the status; frozen once `done` */
  elapsed: number
  /** the status line under the window */
  status: string
  /** the race is over for this side */
  done?: boolean
  /** trailing slot in the status row (the play/pause control) */
  aside?: ReactNode
  children: ReactNode
  className?: string
}

/**
 * One side of the race as a product window: title bar with the three dots and
 * a label, the pane, the spend meter along the bottom, and a status line
 * beneath that ends the race. The iii side carries a ring so the pair reads
 * as "theirs" and "ours" before a word is read.
 */
export function RaceWindow({
  label,
  icon,
  labelHidden,
  tokens,
  versus,
  elapsed,
  emphasis,
  status,
  done,
  aside,
  children,
  className,
}: RaceWindowProps) {
  return (
    <figure className={cn("flex min-w-0 flex-col", className)}>
      <div
        className={cn(
          // A replay, not content: not selectable, so a click never leaves a highlight behind on the typing text.
          "flex h-[420px] flex-col overflow-hidden rounded-[18px] bg-gray-2 select-none min-[720px]:h-[clamp(460px,calc(100dvh-400px),600px)]",
          emphasis ? "shadow-[var(--shadow-panel),inset_0_0_0_1px_var(--gray-8)]" : "shadow-panel",
          done && "shadow-[var(--shadow-panel),inset_0_0_0_1px_var(--gray-11)]",
          "transition-[box-shadow] duration-500 ease-out",
        )}
      >
        <div className="flex h-14 shrink-0 items-center gap-4 border-b border-gray-5 px-5">
          <div aria-hidden="true" className="flex gap-1.5">
            <span className="size-2.5 rounded-full bg-gray-6" />
            <span className="size-2.5 rounded-full bg-gray-6" />
            <span className="size-2.5 rounded-full bg-gray-6" />
          </div>
          <span className="flex min-w-0 items-center gap-2 text-[13px] font-medium text-gray-12">
            {icon}
            <span className={labelHidden ? "sr-only" : "truncate"}>{label}</span>
          </span>
        </div>
        {children}
        <CostMeter tokens={tokens} max={MAX_TOK} emphasis={emphasis} versus={versus} />
      </div>
      <figcaption className="mt-3 flex min-h-9 items-center gap-3 px-1 text-[13px] text-gray-10">
        <span
          aria-hidden="true"
          className={cn(
            "size-1.5 shrink-0 rounded-full transition-colors duration-200 ease-out",
            done ? "bg-gray-12" : "bg-gray-8",
          )}
        />
        <Swap id={status}>{status}</Swap>
        {/* The stopwatch: keeps counting on the side still working, stops on the side that shipped. */}
        <span className={cn("tabular-nums", done ? "text-gray-12" : "text-gray-9")}>
          {done ? `done in ${Math.round(elapsed)}s` : `${Math.floor(elapsed)}s`}
        </span>
        {aside && <span className="ml-auto">{aside}</span>}
      </figcaption>
    </figure>
  )
}
