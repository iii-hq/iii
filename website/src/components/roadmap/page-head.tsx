import type { ReactNode } from "react"
import { cn } from "@/lib/utils"

type PageHeadProps = {
  eyebrow?: string
  title: ReactNode
  /** id for the h1 so the page region can be labelled by it */
  titleId?: string
  /** the one paragraph under the title */
  description?: ReactNode
  /** anything that follows the paragraph (meta line, actions) */
  children?: ReactNode
  className?: string
}

/**
 * The page-level twin of `SectionIntro`: same eyebrow / title / paragraph
 * geometry, but the title is the document's `<h1>`. (SectionIntro hard-codes
 * `<h2>`; once it takes an `as` prop this can go.)
 */
export function PageHead({ eyebrow, title, titleId, description, children, className }: PageHeadProps) {
  return (
    <header className={cn("max-w-[640px]", className)}>
      {eyebrow && <p className="text-[13px] font-medium tracking-[0.02em] text-gray-10">{eyebrow}</p>}
      <h1
        id={titleId}
        className="mt-3 text-[clamp(28px,3.4vw,40px)] leading-[1.1] font-medium tracking-[-0.03em] text-balance text-gray-12"
      >
        {title}
      </h1>
      {description && (
        <p className="mt-4 max-w-[560px] text-[15px] leading-[1.6] text-pretty text-gray-11">{description}</p>
      )}
      {children}
    </header>
  )
}
