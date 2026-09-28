import type { ReactNode } from "react"
import { cn } from "@/lib/utils"

/**
 * A framed step inside the Slack window (code, terminal, console). A "well" in
 * the page colour with a hairline, so it reads as an attachment on the chat
 * surface rather than another window.
 */
export function StepCard({ className, children }: { className?: string; children: ReactNode }) {
  return (
    <div className={cn("overflow-hidden rounded-[12px] bg-gray-1 shadow-[inset_0_0_0_1px_var(--gray-6)]", className)}>
      {children}
    </div>
  )
}

/** Title strip of a StepCard: a label on the left, an optional action slot on the right. */
export function StepCardHeader({ className, children }: { className?: string; children: ReactNode }) {
  return (
    <div
      className={cn(
        "flex h-11 items-center gap-2 border-b border-gray-5 pr-2 pl-4 text-[13px] text-gray-11",
        className,
      )}
    >
      {children}
    </div>
  )
}
