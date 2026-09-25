"use client"

import type { ComponentProps } from "react"
import { SiteLink } from "@/components/site/site-link"
import { track } from "@/lib/analytics"

type Params = Record<string, string | number | boolean | undefined>

type TrackedLinkProps = ComponentProps<"a"> & {
  href: string
  /** `cta_click` params sent on click */
  cta: Params
}

/** A site link that fires a `cta_click` event on click. Lets server components keep their links. */
export function TrackedLink({ href, cta, onClick, children, ...props }: TrackedLinkProps) {
  return (
    <SiteLink
      {...props}
      href={href}
      onClick={(e) => {
        track("cta_click", cta)
        onClick?.(e)
      }}
    >
      {children}
    </SiteLink>
  )
}
