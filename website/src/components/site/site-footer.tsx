import { AskAi } from "@/components/site/ask-ai"
import { ThemeToggle } from "@/components/site/header/theme-toggle"
import { ArrowUpRightIcon } from "@/components/site/iconly"
import { LogoMark } from "@/components/site/logo"
import { TrackedLink } from "@/components/site/tracked-link"
import { ctaLabel } from "@/lib/cta-label"
import { links } from "@/lib/site"
import { cn } from "@/lib/utils"

type FooterLink = {
  label: string
  href: string
  /** opens in a new tab, with a small ↗ and a spoken hint */
  external?: boolean
  /** the old label, kept so `cta_label` in analytics doesn't change */
  trackAs?: string
}

const columns: { title: string; links: FooterLink[] }[] = [
  {
    title: "Product",
    links: [
      { label: "Docs", href: links.docs },
      { label: "Quickstart", href: links.quickstart },
      { label: "Roadmap", href: links.roadmap },
      { label: "Worker registry", href: links.workerRegistry, external: true },
    ],
  },
  {
    title: "Company",
    links: [
      { label: "Manifesto", href: links.manifesto },
      { label: "Blog", href: links.blog },
      { label: "Privacy", href: links.privacy },
    ],
  },
  {
    title: "Community",
    links: [
      { label: "GitHub", href: links.github, external: true },
      { label: "Discord", href: links.discord, external: true },
      { label: "X", href: links.twitter, external: true, trackAs: "twitter / X" },
      { label: "LinkedIn", href: links.linkedin, external: true },
    ],
  },
]

// Text links: 36px tall rows for a comfortable target; the underline draws in
// from the left on hover (background-size is cheap and interruptible) while
// the colour lifts.
const LINK = cn(
  "group/link inline-flex h-9 items-center gap-1 text-[14px] text-gray-11 outline-none",
  "bg-[linear-gradient(currentColor,currentColor)] bg-[length:0%_1px] bg-left-bottom bg-no-repeat",
  "transition-[background-size,color] duration-200 ease-out hover:bg-[length:100%_1px] hover:text-gray-12",
  "rounded-[4px] focus-visible:ring-2 focus-visible:ring-gray-8 focus-visible:ring-offset-2 focus-visible:ring-offset-gray-1",
)

/**
 * Footer: the mark and what iii is, three link columns, then the small print
 * with the "ask an assistant" marks and the theme control. Nothing decorative
 * below the last line; the page just ends.
 */
export function SiteFooter() {
  const year = new Date().getFullYear()

  return (
    <footer className="border-t border-gray-5">
      <div className="mx-auto max-w-[1200px] px-5 pt-16 pb-8 md:px-6">
        <div className="grid grid-cols-2 gap-x-8 gap-y-12 sm:grid-cols-3 lg:grid-cols-[minmax(0,1.6fr)_repeat(3,minmax(0,1fr))]">
          <div className="col-span-2 sm:col-span-3 lg:col-span-1">
            <LogoMark className="size-7 text-gray-12" />
            <p className="mt-6 max-w-[28ch] text-[15px] leading-[1.6] text-pretty text-gray-11">
              The open-source engine every service and AI agent plugs into.
            </p>
            <p className="mt-3 text-[13px] text-gray-10">Pronounced “three eye”.</p>
          </div>

          {columns.map((col) => (
            <nav key={col.title} aria-label={col.title}>
              <h2 className="text-[13px] font-medium text-gray-12">{col.title}</h2>
              <ul className="mt-2 flex flex-col">
                {col.links.map((link) => (
                  <li key={link.href}>
                    <TrackedLink
                      href={link.href}
                      {...(link.external && { target: "_blank", rel: "noopener noreferrer" })}
                      cta={{
                        cta_id: "footer_link",
                        cta_location: "footer",
                        cta_label: ctaLabel(link.trackAs ?? link.label),
                        cta_href: link.href,
                      }}
                      className={LINK}
                    >
                      {link.label}
                      {link.external && (
                        <>
                          <ArrowUpRightIcon className="size-3 text-gray-9 transition-colors duration-200 ease-out group-hover/link:text-gray-12" />
                          <span className="sr-only">(opens in a new tab)</span>
                        </>
                      )}
                    </TrackedLink>
                  </li>
                ))}
              </ul>
            </nav>
          ))}
        </div>

        {/* Phones: the assistant row on its own line, then copyright left and theme right.
            Wider: copyright left, assistants and theme grouped right. */}
        <div className="mt-14 grid grid-cols-2 gap-x-6 gap-y-5 border-t border-gray-5 pt-6 text-[13px] text-gray-10 sm:flex sm:items-center">
          <AskAi className="order-1 col-span-2 sm:order-2 sm:ml-auto" />
          <p className="order-2 self-center sm:order-1">© {year} Motia LLC</p>
          <div className="order-3 flex items-center gap-5 justify-self-end sm:order-3">
            <span aria-hidden="true" className="h-5 w-px bg-gray-5 max-sm:hidden" />
            <ThemeToggle />
          </div>
        </div>
      </div>
    </footer>
  )
}
