'use client'

import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useState } from 'react'

import { Button } from '@/components/ui/button'
import { type Consent, readConsent, writeConsent } from '@/lib/analytics'
import { duration, easeOut } from '@/lib/motion'
import { links } from '@/lib/site'

/**
 * Cookie consent: one line with a privacy link and two buttons, bottom centre. Same consent contract as the previous
 * site (`iii_cookie_consent` in localStorage), so a visitor who already chose is not asked again.
 */
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
      {open ? (
        <motion.div
          role="region"
          aria-label="Cookie consent"
          className="fixed inset-x-0 bottom-0 z-[100] flex justify-center p-4 sm:p-6"
          initial={{ opacity: 0, transform: 'translateY(16px)' }}
          animate={{ opacity: 1, transform: 'translateY(0px)' }}
          exit={{ opacity: 0, transform: 'translateY(16px)', transition: { duration: duration.base } }}
          transition={{ duration: duration.slow, ease: easeOut }}
        >
          <div className="flex w-full max-w-[600px] items-center gap-4 rounded-2xl border bg-popover py-3 pr-3 pl-4 text-[13px] text-muted-foreground leading-normal shadow-[0_12px_32px_-12px_oklch(0_0_0/0.6)] max-sm:flex-col max-sm:items-stretch">
            <p className="m-0 flex-1 text-pretty">
              This site uses cookies to understand how visitors find us.{' '}
              <span className="text-foreground">You can accept or decline non-essential cookies.</span>{' '}
              <a
                href={links.privacy}
                className="rounded-sm text-foreground underline decoration-line-strong underline-offset-[3px] transition-colors hover:decoration-foreground focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-2"
              >
                Privacy policy
              </a>
            </p>
            <div className="flex shrink-0 gap-2 max-sm:justify-end">
              <Button variant="ghost" size="sm" onClick={() => choose('rejected')}>
                Decline
              </Button>
              <Button size="sm" onClick={() => choose('accepted')}>
                Accept
              </Button>
            </div>
          </div>
        </motion.div>
      ) : null}
    </AnimatePresence>
  )
}
