"use client"

import { AnimatePresence, m } from "motion/react"
import { useEffect, useState } from "react"
import { SiteLink } from "@/components/site/site-link"
import { Button } from "@/components/ui/button"
import { type Consent, readConsent, writeConsent } from "@/lib/analytics"
import { links } from "@/lib/site"

/** Cookie consent: one line with a privacy link, two buttons, bottom centre. Same consent contract as the Astro site. */
export function CookieBanner() {
  const [open, setOpen] = useState(false)

  // Consent lives in localStorage, so the banner can only decide after mount.
  useEffect(() => {
    if (readConsent() === null) setOpen(true)
  }, [])

  function choose(value: Consent) {
    writeConsent(value)
    setOpen(false)
  }

  return (
    <AnimatePresence>
      {open && (
        <m.div
          role="region"
          aria-label="Cookie consent"
          className="fixed inset-x-0 bottom-0 z-[100] flex justify-center p-4 sm:p-6"
          initial={{ opacity: 0, y: 16 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: 16, transition: { duration: 0.2 } }}
          transition={{ duration: 0.35, ease: [0.23, 1, 0.32, 1] }}
        >
          <div className="flex w-full max-w-[600px] items-center gap-4 rounded-[14px] bg-gray-2 py-3 pr-3 pl-4 text-[13px] leading-[1.5] text-gray-11 shadow-panel max-sm:flex-col max-sm:items-stretch">
            <p className="m-0 flex-1">
              This site uses cookies to understand how visitors find us.{" "}
              <span className="text-gray-12">You can accept or decline non-essential cookies.</span>{" "}
              <SiteLink
                href={links.privacy}
                className="text-gray-12 underline decoration-gray-7 underline-offset-[3px] transition-colors duration-150 ease-out hover:decoration-gray-12"
              >
                Privacy policy
              </SiteLink>
            </p>
            <div className="flex shrink-0 gap-2 max-sm:justify-end">
              <Button variant="ghost" size="sm" onClick={() => choose("rejected")}>
                Decline
              </Button>
              <Button size="sm" onClick={() => choose("accepted")}>
                Accept
              </Button>
            </div>
          </div>
        </m.div>
      )}
    </AnimatePresence>
  )
}
