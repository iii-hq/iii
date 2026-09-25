"use client"

import { m } from "motion/react"
import { track } from "@/lib/analytics"
import { ctaLabel } from "@/lib/cta-label"
import { cn } from "@/lib/utils"
import type { SectionLink } from "./nav-links"
import { scrollToSection } from "./scroll-to"

type SectionRailProps = {
  links: SectionLink[]
  active: string | null
}

/**
 * In-page section links, shown under the header once the hero is behind you.
 * The active marker is one element sliding between links (shared layoutId).
 */
export function SectionRail({ links, active }: SectionRailProps) {
  return (
    <nav aria-label="Page sections" className="mx-auto max-w-[1200px] px-5 md:px-6">
      <ul className="-mx-2.5 flex h-11 items-center gap-1 overflow-x-auto [scrollbar-width:none]">
        {links.map((link) => {
          const isActive = active === link.id
          return (
            <li key={link.id} className="relative shrink-0">
              <a
                href={`#${link.id}`}
                aria-current={isActive ? "location" : undefined}
                onClick={(e) => {
                  track("cta_click", {
                    cta_id: "scroll_nav_link",
                    cta_location: "scroll_nav",
                    cta_label: ctaLabel(link.label),
                    cta_href: `#${link.id}`,
                  })
                  if (scrollToSection(link.id)) e.preventDefault()
                }}
                className={cn(
                  "flex h-11 items-center px-2.5 text-[13px] outline-none transition-colors duration-150 ease-[ease] focus-visible:text-gray-12",
                  isActive ? "text-gray-12" : "text-gray-10 hover:text-gray-12",
                )}
              >
                {link.label}
              </a>
              {isActive && (
                <m.span
                  layoutId="rail-active"
                  aria-hidden="true"
                  className="absolute inset-x-2.5 bottom-0 h-px bg-gray-12"
                  transition={{ type: "spring", duration: 0.35, bounce: 0 }}
                />
              )}
            </li>
          )
        })}
      </ul>
    </nav>
  )
}
