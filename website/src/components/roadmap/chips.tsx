import { cn } from "@/lib/utils"

const CHIP =
  "inline-flex items-center rounded-[6px] px-1.5 py-0.5 text-[11px] leading-[1.5] font-medium whitespace-nowrap shadow-[inset_0_0_0_1px_var(--gray-5)]"

/** A spec tag: hairline only, so a row of them stays quiet. */
export function TagChip({ children, className }: { children: string; className?: string }) {
  return <span className={cn(CHIP, "text-gray-10", className)}>{children}</span>
}

/** "Interactive": the spec ships a deck. Filled, so it reads as a state and not a tag. */
export function DeckChip({ className }: { className?: string }) {
  return <span className={cn(CHIP, "bg-gray-3 text-gray-11", className)}>Interactive</span>
}
