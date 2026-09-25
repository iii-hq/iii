"use client"

import { AnimatePresence, m } from "motion/react"
import { useState } from "react"
import { ArrowUpRightIcon } from "@/components/site/iconly"
import { SiteLink } from "@/components/site/site-link"
import { track } from "@/lib/analytics"
import { ctaLabel } from "@/lib/cta-label"
import { cn } from "@/lib/utils"
import { navLinks } from "./nav-links"

/**
 * Desktop destination links. A single highlight glides between links on hover
 * or keyboard focus (one shared layoutId), and fades out when the pointer leaves
 * the group, so moving across the menu reads as one continuous gesture.
 */
export function NavMenu({ className }: { className?: string }) {
  const [hovered, setHovered] = useState<string | null>(null)

  return (
    <ul className={cn("flex items-center", className)} onMouseLeave={() => setHovered(null)}>
      {navLinks.map((link) => (
        <li key={link.href} className="relative">
          <SiteLink
            href={link.href}
            {...(link.external && {
              target: "_blank",
              rel: "noopener noreferrer",
              "aria-label": `${link.label}, opens in a new tab`,
            })}
            onMouseEnter={() => setHovered(link.href)}
            onFocus={() => setHovered(link.href)}
            onBlur={() => setHovered(null)}
            onClick={() =>
              track("cta_click", {
                cta_id: "nav_link",
                cta_location: "nav",
                cta_label: ctaLabel(link.label),
                cta_href: link.href,
              })
            }
            className="relative z-10 flex h-8 items-center gap-1 rounded-[8px] px-3 text-sm text-gray-11 outline-none transition-colors duration-150 ease-[ease] hover:text-gray-12 focus-visible:text-gray-12"
          >
            {link.label}
            {link.external && <ArrowUpRightIcon className="size-3 opacity-60" />}
          </SiteLink>
          <AnimatePresence>
            {hovered === link.href && (
              <m.span
                layoutId="nav-hover"
                aria-hidden="true"
                className="absolute inset-0 rounded-[8px] bg-gray-3"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0, transition: { duration: 0.12 } }}
                transition={{ type: "spring", duration: 0.3, bounce: 0 }}
              />
            )}
          </AnimatePresence>
        </li>
      ))}
    </ul>
  )
}
