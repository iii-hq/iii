import type { ReactNode } from "react"
import { cn } from "@/lib/utils"

type SectionIntroProps = {
  /** small label above the title, e.g. "Agents" (matches the header rail) */
  eyebrow?: string
  title: ReactNode
  children?: ReactNode
  /** id for the heading so the section can be labelled by it */
  titleId?: string
  className?: string
}

/**
 * The redesigned sections' header: eyebrow, title, one short paragraph.
 * Left-aligned on the leading edge like the hero, capped at a readable measure.
 */
export function SectionIntro({ eyebrow, title, children, titleId, className }: SectionIntroProps) {
  return (
    // `data-llms` marks the prose scripts/generate-llms-agents.ts extracts from each section.
    <div data-llms="intro" className={cn("max-w-[640px]", className)}>
      {eyebrow && <p className="text-[13px] font-medium tracking-[0.02em] text-gray-10">{eyebrow}</p>}
      <h2
        id={titleId}
        className="mt-3 text-[clamp(28px,3.4vw,40px)] leading-[1.1] font-medium tracking-[-0.03em] text-balance text-gray-12"
      >
        {title}
      </h2>
      {children && <p className="mt-4 max-w-[560px] text-[15px] leading-[1.6] text-pretty text-gray-11">{children}</p>}
    </div>
  )
}
