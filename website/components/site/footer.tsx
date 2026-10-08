import { ArrowUpRightIcon } from 'lucide-react'
import type { ComponentType } from 'react'

import { IconChatGpt, IconClaude, IconGrok, IconPerplexity } from '@/components/icons/assistants'
import { IconDiscord, IconGitHub, IconLinkedIn, IconX } from '@/components/icons/iconly'
import { askAbout, links } from '@/lib/site'
import { cn } from '@/lib/utils'
import { Logo } from './logo'

type Icon = ComponentType<{ className?: string }>
type Item = { label: string; href: string; icon?: Icon; external?: boolean }

const assistantIcons: Record<(typeof askAbout)[number]['id'], Icon> = {
  chatgpt: IconChatGpt,
  claude: IconClaude,
  perplexity: IconPerplexity,
  grok: IconGrok,
}

const rowClass =
  'group/link flex min-h-9 items-center gap-2.5 rounded-md text-[14px] text-foreground/75 outline-none transition-colors hover:text-foreground focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-4'

/**
 * Footer: brand and the big pixel mark on the left, three link columns on the right (product, community with live
 * counts, assistants that can read llms.txt), and a legal row. Same destinations as iii.dev's footer plus the nav's.
 */
/** `container` replaces the default 1240px column so a wider page (the landing) lines its footer up. */
export function Footer({ container }: { container?: string } = {}) {
  const columns: { title: string; items: Item[] }[] = [
    {
      title: 'Product',
      items: [
        { label: 'Docs', href: links.docs },
        { label: 'Quickstart', href: links.quickstart },
        { label: 'Worker registry', href: links.registry, external: true },
        { label: 'Roadmap', href: links.roadmap },
        { label: 'Manifesto', href: links.manifesto },
        { label: 'Blog', href: links.blog },
      ],
    },
    {
      title: 'Community',
      items: [
        { label: 'GitHub', href: links.github, icon: IconGitHub, external: true },
        { label: 'Discord', href: links.discord, icon: IconDiscord, external: true },
        { label: 'Twitter / X', href: links.x, icon: IconX, external: true },
        { label: 'LinkedIn', href: links.linkedin, icon: IconLinkedIn, external: true },
      ],
    },
    {
      title: 'Ask about iii',
      items: askAbout.map((a) => ({ label: a.label, href: a.href, icon: assistantIcons[a.id], external: true })),
    },
  ]

  return (
    <footer className="border-t">
      <div className={cn(container ?? 'mx-auto max-w-[1240px] px-5 sm:px-6 md:px-5', 'pt-14 md:pt-16')}>
        <div className="grid gap-12 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)] lg:gap-20">
          <div className="flex flex-col">
            <a
              href="#hero"
              aria-label="iii, back to top"
              className="inline-flex w-fit items-center rounded-sm outline-none focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-4"
            >
              <Logo className="h-7 text-foreground" />
            </a>
            <p className="mt-4 max-w-[320px] text-pretty text-[15px] text-muted-foreground leading-relaxed">
              A next-generation software system. Workers. Triggers. Functions.
            </p>
            <p className="mt-2 font-sans text-[12px] text-muted-foreground">
              Pronounced <span className="text-foreground/80">&quot;three eye&quot;</span>
            </p>
          </div>

          <nav aria-label="Footer" className="grid grid-cols-2 gap-x-8 gap-y-10 sm:grid-cols-3">
            {columns.map((column) => (
              <div key={column.title} className="min-w-0">
                <h3 className="mb-3 font-medium font-sans text-[12px] text-muted-foreground uppercase tracking-[0.08em]">
                  {column.title}
                </h3>
                <ul className="flex flex-col">
                  {column.items.map((item) => (
                    <li key={item.label}>
                      <a
                        href={item.href}
                        className={rowClass}
                        {...(item.external ? { target: '_blank', rel: 'noopener noreferrer' } : {})}
                      >
                        {item.icon ? (
                          <item.icon className="size-4 shrink-0 text-muted-foreground transition-colors group-hover/link:text-foreground" />
                        ) : null}
                        <span className="truncate">{item.label}</span>
                        {item.external ? (
                          <ArrowUpRightIcon
                            aria-hidden
                            className="ml-auto size-3 shrink-0 -translate-x-1 text-muted-foreground opacity-0 transition-[opacity,translate] duration-150 group-hover/link:translate-x-0 group-hover/link:opacity-100 motion-reduce:transition-none"
                          />
                        ) : null}
                      </a>
                    </li>
                  ))}
                </ul>
              </div>
            ))}
          </nav>
        </div>

        <div className="mt-12 flex items-center border-t py-5 font-sans text-[12px] text-muted-foreground">
          <p className="flex items-center gap-1.5">
            © {new Date().getFullYear()} Motia LLC
            <span aria-hidden>·</span>
            <a
              href={links.privacy}
              className="rounded-sm underline-offset-4 transition-colors hover:text-foreground hover:underline focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-4"
            >
              Privacy
            </a>
          </p>
        </div>
      </div>
    </footer>
  )
}
