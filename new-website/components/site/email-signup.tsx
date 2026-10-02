'use client'

import { ArrowRightIcon } from 'lucide-react'
import { AnimatePresence, motion } from 'motion/react'
import { useId, useState } from 'react'

import { IconTickSquare } from '@/components/icons/iconly'
import { duration, easeOut } from '@/lib/motion'
import { cn } from '@/lib/utils'

const STORAGE_KEY = 'iii_access_email'

/**
 * Updates signup, the same flow as iii.dev's footer form: POST `{ email }` to the Mailmodo form URL
 * (`NEXT_PUBLIC_MAILMODO_FORM_URL`), treat 409 "already subscribed" as success, and never block the
 * user on a network failure. The submitted state is remembered in localStorage.
 */
export function EmailSignup({ className }: { className?: string }) {
  const id = useId()
  const [email, setEmail] = useState('')
  const [state, setState] = useState<'idle' | 'sending' | 'done' | 'invalid'>('idle')

  async function submit(e: React.FormEvent<HTMLFormElement>) {
    e.preventDefault()
    const value = email.trim()
    if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(value)) {
      setState('invalid')
      return
    }
    setState('sending')
    try {
      localStorage.setItem(STORAGE_KEY, value)
    } catch {}
    const url = process.env.NEXT_PUBLIC_MAILMODO_FORM_URL
    if (url) {
      try {
        const res = await fetch(url, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ email: value }),
        })
        if (!res.ok && res.status !== 409) throw new Error('signup failed')
      } catch {
        // Same as the live site: don't block the user if the provider is down.
      }
    }
    setState('done')
  }

  return (
    <form
      onSubmit={submit}
      noValidate
      aria-describedby={`${id}-status`}
      className={cn('relative h-11 w-full min-w-0', className)}
    >
      <AnimatePresence initial={false} mode="wait">
        {state === 'done' ? (
          <motion.p
            key="done"
            role="status"
            initial={{ opacity: 0, y: 4 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: duration.base, ease: easeOut }}
            className="flex h-full items-center gap-2 rounded-xl border bg-card px-3.5 text-[14px] text-foreground"
          >
            <IconTickSquare className="size-4 text-ok" />
            Thanks for subscribing.
          </motion.p>
        ) : (
          <motion.div
            key="form"
            exit={{ opacity: 0, y: -4 }}
            transition={{ duration: duration.fast }}
            className={cn(
              'flex h-full items-center rounded-xl border bg-card pr-1 pl-3.5 transition-colors focus-within:border-line-strong',
              state === 'invalid' && 'border-fail/60',
            )}
          >
            <label htmlFor={`${id}-email`} className="sr-only">
              Email for updates
            </label>
            <input
              id={`${id}-email`}
              type="email"
              name="email"
              inputMode="email"
              autoComplete="email"
              placeholder="your@email.here"
              value={email}
              onChange={(e) => {
                setEmail(e.target.value)
                if (state === 'invalid') setState('idle')
              }}
              aria-invalid={state === 'invalid'}
              className="h-full min-w-0 flex-1 bg-transparent font-sans text-[13px] text-foreground placeholder:text-muted-foreground focus:outline-none"
            />
            <button
              type="submit"
              aria-label="Subscribe for updates"
              disabled={state === 'sending'}
              className="pressable flex size-9 shrink-0 items-center justify-center rounded-lg text-muted-foreground outline-none hover:bg-foreground/[0.06] hover:text-foreground focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-2 disabled:opacity-50"
            >
              <ArrowRightIcon aria-hidden className="size-4" strokeWidth={1.75} />
            </button>
          </motion.div>
        )}
      </AnimatePresence>
      <output id={`${id}-status`} aria-live="polite" className="sr-only">
        {state === 'invalid' ? 'Enter a valid email address.' : state === 'done' ? 'Subscribed.' : ''}
      </output>
    </form>
  )
}
