import { useEffect, useState } from 'react'
import { ModeToggle } from '@/components/schematic/ModeToggle'
import { Wordmark } from '@/components/schematic/Wordmark'
import { DECK_META, NAV } from '@/content/deck'
import type { Route } from '@/hooks/useHashRoute'
import { useTheme } from '@/hooks/useTheme'
import { cn } from '@/lib/utils'

export function TopNav({ route }: { route: Route }) {
  const [theme, setTheme] = useTheme()
  const [active, setActive] = useState<string | null>(null)

  // scroll-spy: highlight the section currently in view (home page only)
  useEffect(() => {
    if (route.kind !== 'home') return
    const sections = NAV.map((l) =>
      document.getElementById(l.id),
    ).filter((el): el is HTMLElement => el !== null)
    if (sections.length === 0) return
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) setActive(entry.target.id)
        }
      },
      { rootMargin: '-20% 0px -70% 0px' },
    )
    for (const el of sections) observer.observe(el)
    return () => observer.disconnect()
  }, [route])

  return (
    <header className="sticky top-0 z-50 border-b border-rule bg-bg">
      <div className="flex items-center gap-x-5 px-4 py-2.5 @3xl:px-9">
        <a
          href="#/"
          className="flex items-center gap-x-2.5 shrink-0"
          aria-label="iii — back to overview"
        >
          <Wordmark />
          <span className="font-mono text-[13px] font-semibold lowercase text-ink hidden @lg:inline">
            {DECK_META.wordmarkLabel}
          </span>
        </a>

        {route.kind === 'home' ? (
          <nav className="hidden @3xl:flex items-center gap-x-4 min-w-0 overflow-x-auto">
            {NAV.map((link) => (
              <a
                key={link.id}
                href={`#${link.id}`}
                className={cn(
                  'font-mono text-[12px] lowercase py-1.5 transition-colors whitespace-nowrap',
                  active === link.id
                    ? 'text-accent'
                    : 'text-ink-faint hover:text-ink',
                )}
              >
                {link.label}
              </a>
            ))}
          </nav>
        ) : (
          <a
            href="#/"
            className="font-mono text-[12px] lowercase text-ink-faint hover:text-ink transition-colors"
          >
            ← back to the overview
          </a>
        )}

        <div className="ml-auto flex items-center gap-x-4 shrink-0">
          <a
            href="#/spec"
            className={cn(
              'font-mono text-[12px] lowercase whitespace-nowrap transition-colors',
              route.kind === 'page' && route.slug === 'spec'
                ? 'text-accent'
                : 'text-ink-faint hover:text-ink',
            )}
          >
            spec
          </a>
          <ModeToggle
            value={theme}
            onChange={setTheme}
            options={[
              { value: 'light', label: 'light' },
              { value: 'dark', label: 'dark' },
            ]}
          />
        </div>
      </div>
    </header>
  )
}
