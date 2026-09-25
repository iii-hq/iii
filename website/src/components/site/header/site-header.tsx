"use client"

import { AnimatePresence, m, useMotionValueEvent, useReducedMotion, useScroll } from "motion/react"
import { usePathname } from "next/navigation"
import { useEffect, useState } from "react"
import { LogoMark } from "@/components/site/logo"
import { SiteLink } from "@/components/site/site-link"
import { cn } from "@/lib/utils"
import { CommunityLinks } from "./community-links"
import { GetStartedButton } from "./cta-buttons"
import { MobileNav } from "./mobile-nav"
import { LANDING_HERO_ID, landingSections, type SectionLink } from "./nav-links"
import { NavMenu } from "./nav-menu"
import { SectionRail } from "./section-rail"
import { SocialsMenu } from "./socials-menu"
import { ThemeToggle } from "./theme-toggle"

const NO_SECTIONS: SectionLink[] = []

/**
 * Fixed header: mark on the left, everything else right-aligned ending in one
 * solid action. Rendered once in the root layout, so it stays mounted across
 * route changes and nothing in it re-fetches or re-measures. On the landing
 * page it is transparent over the hero; once the page moves it gains a frosted
 * surface and a hairline, and after the hero scrolls away the section rail
 * drops in beneath it with a scroll-spy. Every other page has no hero, so the
 * surface is on from the start.
 */
export function SiteHeader() {
  const landing = usePathname() === "/"
  const sections = landing ? landingSections : NO_SECTIONS
  const revealAfter = landing ? LANDING_HERO_ID : undefined
  const reduce = useReducedMotion()
  const [scrolled, setScrolled] = useState(false)
  const [pastHero, setPastHero] = useState(false)
  const [active, setActive] = useState<string | null>(null)

  const { scrollY } = useScroll()
  useMotionValueEvent(scrollY, "change", (y) => setScrolled(y > 8))
  useEffect(() => setScrolled(window.scrollY > 8), [])

  // The hero and sections belong to other components; their ids are the contract.
  useEffect(() => {
    if (!revealAfter) {
      setPastHero(false)
      return
    }
    const hero = document.getElementById(revealAfter)
    if (!hero) return
    const observer = new IntersectionObserver(([entry]) => setPastHero(!entry.isIntersecting), {
      rootMargin: "-64px 0px 0px 0px",
    })
    observer.observe(hero)
    return () => observer.disconnect()
  }, [revealAfter])

  useEffect(() => {
    setActive(null)
    const els = sections.map((s) => document.getElementById(s.id)).filter((el): el is HTMLElement => el !== null)
    if (!els.length) return
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) if (entry.isIntersecting) setActive(entry.target.id)
      },
      { rootMargin: "-20% 0px -70% 0px" },
    )
    for (const el of els) observer.observe(el)
    return () => observer.disconnect()
  }, [sections])

  // Inner pages have no hero to be transparent over: the surface is on from the start.
  const solid = scrolled || pastHero || !revealAfter

  return (
    <header className="fixed inset-x-0 top-0 z-50">
      {/* Frosted surface. Opacity-only transition so it stays on the compositor. */}
      <div
        aria-hidden="true"
        className={cn(
          "absolute inset-0 border-b border-line bg-gray-1/80 backdrop-blur-xl transition-opacity duration-200 ease-out",
          solid ? "opacity-100" : "opacity-0",
        )}
      />

      <div className="relative mx-auto flex h-16 max-w-[1200px] items-center px-5 md:px-6">
        <SiteLink
          href="/"
          aria-label="iii home"
          className="-ml-1.5 flex size-9 items-center justify-center rounded-[8px] text-gray-12 outline-none focus-visible:ring-2 focus-visible:ring-gray-8"
        >
          <LogoMark className="size-[22px]" />
        </SiteLink>

        <div className="ml-auto flex items-center gap-1">
          <NavMenu className="max-lg:hidden" />
          <SocialsMenu className="max-lg:hidden" />
          <CommunityLinks location="nav" className="max-md:hidden" />
          <ThemeToggle className="max-lg:hidden" />
          <span aria-hidden="true" className="mx-2 h-4 w-px bg-gray-6 max-sm:hidden" />
          <GetStartedButton location="nav" className="max-sm:hidden" />
          <MobileNav />
        </div>
      </div>

      <AnimatePresence initial={false}>
        {pastHero && (
          <m.div
            key="rail"
            className="relative"
            initial={reduce ? { opacity: 0 } : { opacity: 0, transform: "translateY(-6px)" }}
            animate={{ opacity: 1, transform: "translateY(0px)" }}
            exit={
              reduce ? { opacity: 0 } : { opacity: 0, transform: "translateY(-6px)", transition: { duration: 0.15 } }
            }
            transition={{ duration: 0.25, ease: [0.23, 1, 0.32, 1] }}
          >
            <div aria-hidden="true" className="absolute inset-0 border-b border-line bg-gray-1/80 backdrop-blur-xl" />
            <div className="relative">
              <SectionRail links={sections} active={active} />
            </div>
          </m.div>
        )}
      </AnimatePresence>
    </header>
  )
}
