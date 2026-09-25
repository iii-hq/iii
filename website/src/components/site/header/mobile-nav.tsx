"use client"

import { m, useReducedMotion } from "motion/react"
import { useState } from "react"
import { ArrowRightIcon, ArrowUpRightIcon, CloseIcon, MenuIcon } from "@/components/site/iconly"
import { SiteLink } from "@/components/site/site-link"
import { buttonVariants } from "@/components/ui/button-variants"
import { Sheet, SheetClose, SheetContent, SheetTitle, SheetTrigger } from "@/components/ui/sheet"
import { track, trackCta } from "@/lib/analytics"
import { ctaLabel } from "@/lib/cta-label"
import { cn } from "@/lib/utils"
import { GetStartedButton } from "./cta-buttons"
import { navLinks, socialLinks } from "./nav-links"
import { SocialIcon } from "./social-icon"
import { ThemeToggle } from "./theme-toggle"

const EASE_OUT = [0.23, 1, 0.32, 1] as const

/** Hamburger + a top sheet that drops down with the iOS drawer curve; rows arrive on a short stagger. */
export function MobileNav() {
  const [open, setOpen] = useState(false)
  const reduce = useReducedMotion()

  const enter = (i: number) => ({
    initial: reduce ? { opacity: 0 } : { opacity: 0, y: -6 },
    animate: { opacity: 1, y: 0 },
    transition: { duration: 0.3, ease: EASE_OUT, delay: 0.06 + i * 0.03 },
  })

  return (
    <Sheet open={open} onOpenChange={setOpen}>
      <SheetTrigger
        aria-label="Open menu"
        className={cn(buttonVariants({ variant: "ghost", size: "icon-sm" }), "lg:hidden")}
      >
        <MenuIcon className="size-[18px]" />
      </SheetTrigger>
      <SheetContent
        side="top"
        showCloseButton={false}
        overlayClassName="bg-black/40 transition-opacity duration-[300ms] supports-backdrop-filter:backdrop-blur-sm"
        className="gap-0 rounded-b-[20px] border-b-0 bg-gray-2 p-0 shadow-panel transition-[opacity,translate,transform] duration-[300ms] ease-[cubic-bezier(0.32,0.72,0,1)] data-[side=top]:data-starting-style:translate-y-[-100%] data-[side=top]:data-ending-style:translate-y-[-100%] data-ending-style:opacity-100 data-starting-style:opacity-100"
      >
        <div className="flex h-16 items-center justify-between px-5">
          <SheetTitle className="text-sm font-medium text-gray-11">Menu</SheetTitle>
          <SheetClose aria-label="Close menu" className={buttonVariants({ variant: "ghost", size: "icon-sm" })}>
            <CloseIcon className="size-[18px]" />
          </SheetClose>
        </div>

        <ul className="px-3">
          {navLinks.map((link, i) => (
            <m.li key={link.href} {...enter(i)}>
              <SiteLink
                href={link.href}
                {...(link.external && { target: "_blank", rel: "noopener noreferrer" })}
                onClick={() => {
                  track("cta_click", {
                    cta_id: "mobile_menu_link",
                    cta_location: "mobile_menu",
                    cta_label: ctaLabel(link.label),
                    cta_href: link.href,
                  })
                  setOpen(false)
                }}
                className="flex h-12 items-center justify-between rounded-[10px] px-3 text-[17px] font-medium tracking-[-0.01em] text-gray-12 transition-colors duration-150 active:bg-gray-4"
              >
                {link.label}
                {link.external ? (
                  <ArrowUpRightIcon className="size-4 text-gray-9" />
                ) : (
                  <ArrowRightIcon className="size-4 text-gray-9" />
                )}
              </SiteLink>
            </m.li>
          ))}
        </ul>

        <m.p
          {...enter(navLinks.length)}
          className="mt-4 px-6 text-xs font-medium tracking-[0.04em] text-gray-10 uppercase"
        >
          Community
        </m.p>
        <ul className="mt-1 grid grid-cols-2 gap-1 px-3">
          {socialLinks.map((link, i) => (
            <m.li key={link.id} {...enter(navLinks.length + 1 + i)}>
              <a
                href={link.href}
                target="_blank"
                rel="noopener noreferrer"
                onClick={() => {
                  trackCta(link.id, "mobile_menu", { cta_label: link.title.toLowerCase() })
                  setOpen(false)
                }}
                className="flex h-12 items-center gap-3 rounded-[10px] px-3 text-[15px] font-medium text-gray-12 transition-colors duration-150 active:bg-gray-4"
              >
                <SocialIcon id={link.id} className="size-4 text-gray-11" />
                {link.title}
              </a>
            </m.li>
          ))}
        </ul>

        <div className="mt-3 flex items-center justify-between gap-3 border-t border-gray-5 px-5 py-4 pb-[max(1rem,env(safe-area-inset-bottom))]">
          <ThemeToggle />
          <GetStartedButton location="mobile_menu" />
        </div>
      </SheetContent>
    </Sheet>
  )
}
