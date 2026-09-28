/** Header (64px) + section rail (44px) + breathing room. */
export const HEADER_OFFSET = 116

/**
 * Smooth-scroll to an in-page section, landing it below the fixed header.
 * The legacy sections carry their own scroll-margin for a shorter header, so
 * the offset is applied here instead of relying on the browser's anchor jump.
 */
export function scrollToSection(id: string) {
  const el = document.getElementById(id)
  if (!el) return false
  const top = el.getBoundingClientRect().top + window.scrollY - HEADER_OFFSET
  const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches
  window.scrollTo({ top, behavior: reduce ? "auto" : "smooth" })
  history.replaceState(null, "", `#${id}`)
  return true
}
