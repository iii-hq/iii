"use client"

import { buttonVariants } from "@/components/ui/button-variants"
import { trackCta } from "@/lib/analytics"
import { links } from "@/lib/site"
import { cn } from "@/lib/utils"

/** The header's solid action: a link to the install docs. */
export function GetStartedButton({ location, className }: { location: string; className?: string }) {
  return (
    <a
      href={links.install}
      onClick={() => trackCta("get_started", location, { cta_label: "get started" })}
      className={cn(buttonVariants({ size: "sm" }), className)}
    >
      Get started
    </a>
  )
}
