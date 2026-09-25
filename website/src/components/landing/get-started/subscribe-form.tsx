"use client"

import { AnimatePresence, m, useReducedMotion } from "motion/react"
import { type FormEvent, useId, useRef, useState } from "react"
import { CheckIcon } from "@/components/site/iconly"
import { buttonVariants } from "@/components/ui/button-variants"
import { subscribe } from "@/lib/subscribe"
import { cn } from "@/lib/utils"

type SubscribeFormProps = {
  /** analytics `form_location` */
  location: string
  className?: string
}

/**
 * Email signup in the v2 style: a grey field with a real "Subscribe" button.
 * Validates on submit only, says how to fix it beside the field, and swaps to
 * a thank-you once sent. The thank-you lasts for this page view only: on the
 * next load the field is back, so another address can be added.
 */
export function SubscribeForm({ location, className }: SubscribeFormProps) {
  const id = useId()
  const errorId = useId()
  const inputRef = useRef<HTMLInputElement>(null)
  const reduce = useReducedMotion()
  const [email, setEmail] = useState("")
  const [error, setError] = useState<string | null>(null)
  const [submitted, setSubmitted] = useState(false)

  function onSubmit(e: FormEvent) {
    e.preventDefault()
    const value = email.trim()
    if (!value || !inputRef.current?.checkValidity()) {
      setError("Enter an email address like you@example.com")
      inputRef.current?.focus()
      return
    }
    setError(null)
    setSubmitted(true)
    setEmail("")
    void subscribe(value, location)
  }

  return (
    <form onSubmit={onSubmit} noValidate className={cn("min-w-0", className)}>
      <AnimatePresence mode="popLayout" initial={false}>
        {submitted ? (
          <m.p
            key="thanks"
            role="status"
            initial={{ opacity: 0, y: reduce ? 0 : 4 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.2, ease: [0.23, 1, 0.32, 1] }}
            className="flex h-11 items-center gap-2 px-1 text-[14px] text-gray-11"
          >
            <CheckIcon className="size-4 shrink-0 text-gray-12" />
            Thanks for subscribing. Updates will land in your inbox.
          </m.p>
        ) : (
          <m.div key="field" exit={{ opacity: 0, y: reduce ? 0 : -4, transition: { duration: 0.12 } }}>
            <div className="flex gap-2">
              <label htmlFor={id} className="sr-only">
                Email address
              </label>
              <input
                ref={inputRef}
                id={id}
                type="email"
                name="email"
                autoComplete="email"
                inputMode="email"
                placeholder="you@example.com"
                value={email}
                onChange={(e) => {
                  setEmail(e.target.value)
                  if (error) setError(null)
                }}
                aria-invalid={error ? true : undefined}
                aria-describedby={error ? errorId : undefined}
                className={cn(
                  "h-11 min-w-0 flex-1 rounded-[10px] bg-gray-1 px-4 text-[16px] text-gray-12 shadow-[inset_0_0_0_1px_var(--gray-6)] outline-none placeholder:text-gray-9 sm:text-[14px]",
                  "focus-visible:ring-2 focus-visible:ring-gray-8 focus-visible:ring-offset-2 focus-visible:ring-offset-gray-2",
                  "aria-invalid:shadow-[inset_0_0_0_1px_var(--gray-9)]",
                )}
              />
              <button type="submit" className={cn(buttonVariants({ variant: "secondary", size: "lg" }), "shrink-0")}>
                Subscribe
              </button>
            </div>
            {error && (
              <p id={errorId} className="mt-2 px-1 text-[12.5px] text-gray-11">
                {error}
              </p>
            )}
          </m.div>
        )}
      </AnimatePresence>
    </form>
  )
}
