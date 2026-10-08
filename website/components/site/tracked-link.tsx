'use client'

import type { ComponentProps } from 'react'

import { track } from '@/lib/analytics'

type Params = Record<string, string | number | boolean | undefined>

type TrackedLinkProps = ComponentProps<'a'> & {
  href: string
  /** event name, `cta_click` by default */
  event?: string
  /** event params sent on click */
  params: Params
}

/** A link that pushes an analytics event on click (a no-op without cookie consent). Lets server components keep links. */
export function TrackedLink({ href, event = 'cta_click', params, onClick, children, ...props }: TrackedLinkProps) {
  return (
    <a
      {...props}
      href={href}
      onClick={(e) => {
        track(event, params)
        onClick?.(e)
      }}
    >
      {children}
    </a>
  )
}
