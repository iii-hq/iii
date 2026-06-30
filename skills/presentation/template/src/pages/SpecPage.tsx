import { HighlightStyles } from '@/content/highlight'
import { Markdown } from '@/content/markdown'
import { useHashRoute } from '@/hooks/useHashRoute'
import { cn } from '@/lib/utils'

/**
 * A14b - the spec viewer. A separate hash-route page (`#/spec/<file>`) that
 * renders every markdown file of the tech-spec, with a sidebar listing the
 * files and the rendered doc beside it. The spec markdown is bundled at build
 * time from the sibling spec directory (`<spec-dir>/*.md`).
 */

// the spec's markdown lives one level above the presentation dir
const RAW = import.meta.glob('../../../*.md', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>

interface Doc {
  slug: string
  file: string
  label: string
  source: string
}

function firstHeading(src: string): string | null {
  const m = src.match(/^#\s+(.+?)\s*$/m)
  return m ? m[1].replace(/`/g, '').trim() : null
}

const DOCS: Doc[] = Object.entries(RAW)
  .map(([path, source]) => {
    const file = path.split('/').pop() ?? path
    const base = file.replace(/\.md$/, '')
    return { slug: base.toLowerCase(), file, label: firstHeading(source) ?? base, source }
  })
  .filter((d) => !/-review-|^_/.test(d.file))
  .sort((a, b) => {
    if (a.slug === 'readme') return -1
    if (b.slug === 'readme') return 1
    return a.file.localeCompare(b.file)
  })

export function SpecPage() {
  const route = useHashRoute()
  const wanted = route.kind === 'page' ? route.rest[0] : undefined
  const active = DOCS.find((d) => d.slug === wanted) ?? DOCS[0]

  if (!active) {
    return (
      <main className="px-4 py-24 @3xl:px-9">
        <p className="font-mono text-[14px] lowercase text-ink-faint">
          no spec markdown found beside this deck.
        </p>
      </main>
    )
  }

  return (
    <main className="px-4 @3xl:px-9 py-8 @3xl:py-10">
      <HighlightStyles />
      <div className="grid grid-cols-1 @3xl:grid-cols-[210px_minmax(0,1fr)] gap-6 @3xl:gap-10">
        <nav
          aria-label="spec files"
          className="@3xl:sticky @3xl:top-20 @3xl:self-start min-w-0 border-b border-rule pb-3 @3xl:border-b-0 @3xl:pb-0 @3xl:border-r @3xl:border-rule @3xl:pr-6"
        >
          <div className="font-mono text-[10px] uppercase tracking-[0.14em] text-ink-ghost mb-3">
            spec
          </div>
          <ul className="flex @3xl:flex-col gap-x-1 gap-y-0.5 overflow-x-auto @3xl:overflow-visible">
            {DOCS.map((d) => {
              const on = d.slug === active.slug
              return (
                <li key={d.slug} className="shrink-0 @3xl:shrink">
                  <a
                    href={`#/spec/${d.slug}`}
                    aria-current={on ? 'page' : undefined}
                    className={cn(
                      'block whitespace-nowrap @3xl:whitespace-normal font-mono text-[12px] leading-[1.45] py-1.5 px-3 @3xl:pl-3 @3xl:-ml-px @3xl:border-l-2 transition-colors',
                      on
                        ? 'text-ink @3xl:border-accent'
                        : 'text-ink-faint hover:text-ink @3xl:border-transparent',
                    )}
                  >
                    {d.label}
                  </a>
                </li>
              )
            })}
          </ul>
        </nav>

        <article className="min-w-0 max-w-[80ch]">
          <Markdown source={active.source} />
        </article>
      </div>
    </main>
  )
}
