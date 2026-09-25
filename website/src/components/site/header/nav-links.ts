import { links } from "@/lib/site"

export type NavLink = { label: string; href: string; external?: boolean }

/** Destination links in the site header and the mobile menu. */
export const navLinks: NavLink[] = [
  { label: "Manifesto", href: links.manifesto },
  { label: "Docs", href: links.docs },
  { label: "Blog", href: links.blog },
  { label: "Roadmap", href: links.roadmap },
  { label: "Worker registry", href: links.workerRegistry, external: true },
]

export type SectionLink = { id: string; label: string }

/** The landing page's hero id and in-page sections, for the header's section rail. ids match each section's `id`. */
export const LANDING_HERO_ID = "hero"
export const landingSections: SectionLink[] = [
  { id: "console-live", label: "Agents" },
  { id: "harness", label: "Harness" },
  { id: "experience", label: "Why iii" },
  { id: "hello", label: "Languages" },
  { id: "workers", label: "Workers" },
]

export type SocialId = "github" | "discord" | "x" | "linkedin"

export type SocialLink = {
  id: SocialId
  title: string
  /** shown under the title when there's no live stat for it */
  description: string
  href: string
}

/** The community links behind the header's "Community" menu. */
export const socialLinks: SocialLink[] = [
  { id: "github", title: "GitHub", description: "Source, issues and releases", href: links.github },
  { id: "discord", title: "Discord", description: "Chat with the team", href: links.discord },
  { id: "x", title: "X", description: "Formerly Twitter", href: links.twitter },
  { id: "linkedin", title: "LinkedIn", description: "Company updates", href: links.linkedin },
]
