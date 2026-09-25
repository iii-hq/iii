"use client"

import { m } from "motion/react"
import { ArrowRightIcon } from "@/components/site/iconly"
import { cn } from "@/lib/utils"
import styles from "./experience.module.css"
import { EASE_OUT, STEP_ENTER } from "./motion"
import { useAutoAction } from "./wait-action"

type TicketCardProps = {
  ticketId: string
  title: string
  label: string
  count?: number
  onClick: () => void
}

/** The inline work ticket on the first message. Clicking it starts the flow. */
export function TicketCard({ ticketId, title, label, count, onClick }: TicketCardProps) {
  // The first wait point: breathe a ring so it reads as the thing to click, and start on its own after a moment.
  useAutoAction(true, onClick)
  return (
    <m.div {...STEP_ENTER} className={cn(styles.nudge, "mt-3 flex max-w-[560px] rounded-[12px]")}>
      <button
        type="button"
        onClick={onClick}
        aria-label={`${label}. Ticket ${ticketId}, ${title}`}
        className="group/ticket flex w-full cursor-pointer flex-col gap-2 rounded-[12px] bg-gray-1 px-4 py-3.5 text-left shadow-[inset_0_0_0_1px_var(--gray-6)] outline-none transition-[background-color,box-shadow,transform] duration-150 ease-out hover:bg-gray-3 hover:shadow-[inset_0_0_0_1px_var(--gray-7)] focus-visible:ring-2 focus-visible:ring-gray-8 focus-visible:ring-offset-2 focus-visible:ring-offset-gray-2 active:scale-[0.96] motion-reduce:active:scale-100"
      >
        <span className="flex min-w-0 items-center gap-2 text-[12px] text-gray-10">
          <span className="shrink-0 font-medium text-gray-11 tabular-nums whitespace-nowrap">{ticketId}</span>
          <span className="text-gray-7">/</span>
          <span className="truncate">{title}</span>
        </span>
        <span className="flex items-center gap-2 text-[14px] font-medium text-gray-12">
          {label}
          <ArrowRightIcon className="size-4 text-gray-9 transition-transform duration-200 ease-out group-hover/ticket:translate-x-0.5 motion-reduce:transition-none" />
        </span>
      </button>
      {count != null && (
        <m.span
          aria-hidden="true"
          className="pointer-events-none absolute -top-2 -right-2 flex h-5 min-w-5 items-center justify-center rounded-full bg-gray-12 px-1.5 text-[11px] leading-none font-medium text-gray-1 tabular-nums ring-2 ring-gray-2"
          initial={{ opacity: 0, scale: 0.9 }}
          animate={{ opacity: 1, scale: 1 }}
          transition={{ duration: 0.25, delay: 0.9, ease: EASE_OUT }}
        >
          {count}
        </m.span>
      )}
    </m.div>
  )
}
