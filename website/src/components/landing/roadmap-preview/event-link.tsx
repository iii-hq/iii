"use client"

import type { ComponentProps } from "react"
import { SiteLink } from "@/components/site/site-link"
import { track } from "@/lib/analytics"

type EventLinkProps = Omit<ComponentProps<"a">, "href"> & {
  href: string
  event: string
  params?: Record<string, string>
}

/** A site link that fires a named analytics event (not just `cta_click`) on click. */
export function EventLink({ href, event, params, onClick, ...props }: EventLinkProps) {
  return (
    <SiteLink
      {...props}
      href={href}
      onClick={(e) => {
        track(event, params)
        onClick?.(e)
      }}
    />
  )
}
