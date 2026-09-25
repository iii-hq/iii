import Link from "next/link"
import type { ComponentProps } from "react"

export type SiteLinkProps = Omit<ComponentProps<"a">, "href"> & { href: string }

/** Same-site paths (`/blog`, `/roadmap/x`) that the app router serves as pages. */
const isInternalPath = (href: string) => href.startsWith("/") && !href.startsWith("//") && !/\.[a-z0-9]+$/i.test(href)

/**
 * The one anchor for the site. Internal pages go through the app router, so
 * the header, footer and theme stay mounted across a route change instead of
 * the whole document reloading. Everything else is a plain `<a>`.
 */
export function SiteLink({ href, ...props }: SiteLinkProps) {
  return isInternalPath(href) ? <Link href={href} {...props} /> : <a href={href} {...props} />
}
